/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use dom::bindings::codegen::WindowBinding;
use dom::bindings::js::JS;
use dom::bindings::trace::Untraceable;
use dom::bindings::utils::{Reflectable, Reflector};
use dom::document::Document;
use dom::element::Element;
use dom::eventtarget::{EventTarget, WindowTypeId};
use dom::console::Console;
use dom::location::Location;
use dom::navigator::Navigator;

use layout_interface::{ReflowForDisplay, DocumentDamageLevel};
use script_task::{ExitWindowMsg, FireTimerMsg, Page, ScriptChan};
use servo_msg::compositor_msg::ScriptListener;
use servo_net::image_cache_task::ImageCacheTask;
use servo_util::str::DOMString;
use servo_util::task::{spawn_named};

use js::jsapi::{JSObject, JSContext, JS_DefineProperty, JS_PropertyStub, JS_StrictPropertyStub};
use js::jsval::{NullValue, ObjectValue, JSVal};
use js::JSPROP_ENUMERATE;

use collections::hashmap::HashMap;
use std::cmp;
use std::comm::{channel, Sender, Receiver};
use std::comm::Select;
use std::hash::{Hash, sip};
use std::io::timer::Timer;
use std::rc::Rc;

use serialize::{Encoder, Encodable};
use url::Url;

pub enum TimerControlMsg {
    TimerMessageFire(~TimerData),
    TimerMessageClose,
    TimerMessageTriggerExit //XXXjdm this is just a quick hack to talk to the script task
}

pub struct TimerHandle {
    handle: i32,
    cancel_chan: Option<Sender<()>>,
}

impl<S: Encoder> Encodable<S> for TimerHandle {
    fn encode(&self, _: &mut S) {
    }
}

impl Hash for TimerHandle {
    fn hash(&self, state: &mut sip::SipState) {
        self.handle.hash(state);
    }
}

impl Eq for TimerHandle {
    fn eq(&self, other: &TimerHandle) -> bool {
        self.handle == other.handle
    }
}

impl TotalEq for TimerHandle {
    fn equals(&self, other: &TimerHandle) -> bool {
        self.eq(other)
    }
}

impl TimerHandle {
    fn cancel(&self) {
        self.cancel_chan.as_ref().map(|chan| chan.send(()));
    }
}

#[deriving(Encodable)]
pub struct Window {
    eventtarget: EventTarget,
    script_chan: ScriptChan,
    console: Option<JS<Console>>,
    location: Option<JS<Location>>,
    navigator: Option<JS<Navigator>>,
    image_cache_task: ImageCacheTask,
    active_timers: ~HashMap<i32, TimerHandle>,
    next_timer_handle: i32,
    compositor: Untraceable<~ScriptListener>,
    timer_chan: Untraceable<Sender<TimerControlMsg>>,
    page: Rc<Page>,
}

impl Window {
    pub fn get_cx(&self) -> *JSObject {
        let js_info = self.page().js_info();
        js_info.get_ref().js_compartment.deref().cx.deref().ptr
    }

    pub fn page<'a>(&'a self) -> &'a Page {
        &*self.page
    }
    pub fn get_url(&self) -> Url {
        self.page().get_url()
    }
}

#[unsafe_destructor]
impl Drop for Window {
    fn drop(&mut self) {
        self.timer_chan.send(TimerMessageClose);
        for timer_handle in self.active_timers.values() {
            timer_handle.cancel();
        }
    }
}

// Holder for the various JS values associated with setTimeout
// (ie. function value to invoke and all arguments to pass
//      to the function when calling it)
pub struct TimerData {
    handle: i32,
    is_interval: bool,
    funval: JSVal,
    args: ~[JSVal],
}

impl Window {
    pub fn Alert(&self, s: DOMString) {
        // Right now, just print to the console
        println!("ALERT: {:s}", s);
    }

    pub fn Close(&self) {
        self.timer_chan.deref().send(TimerMessageTriggerExit);
    }

    pub fn Document(&self) -> JS<Document> {
        let frame = self.page().frame();
        frame.get_ref().document.clone()
    }

    pub fn Name(&self) -> DOMString {
        ~""
    }

    pub fn SetName(&self, _name: DOMString) {
    }

    pub fn Status(&self) -> DOMString {
        ~""
    }

    pub fn SetStatus(&self, _status: DOMString) {
    }

    pub fn Closed(&self) -> bool {
        false
    }

    pub fn Stop(&self) {
    }

    pub fn Focus(&self) {
    }

    pub fn Blur(&self) {
    }

    pub fn GetFrameElement(&self) -> Option<JS<Element>> {
        None
    }

    pub fn Location(&mut self, abstract_self: &JS<Window>) -> JS<Location> {
        if self.location.is_none() {
            self.location = Some(Location::new(abstract_self, self.page.clone()));
        }
        self.location.get_ref().clone()
    }

    pub fn Console(&mut self, abstract_self: &JS<Window>) -> JS<Console> {
        if self.console.is_none() {
            self.console = Some(Console::new(abstract_self));
        }
        self.console.get_ref().clone()
    }

    pub fn Navigator(&mut self, abstract_self: &JS<Window>) -> JS<Navigator> {
        if self.navigator.is_none() {
            self.navigator = Some(Navigator::new(abstract_self));
        }
        self.navigator.get_ref().clone()
    }

    pub fn Confirm(&self, _message: DOMString) -> bool {
        false
    }

    pub fn Prompt(&self, _message: DOMString, _default: DOMString) -> Option<DOMString> {
        None
    }

    pub fn Print(&self) {
    }

    pub fn ShowModalDialog(&self, _cx: *JSContext, _url: DOMString, _argument: Option<JSVal>) -> JSVal {
        NullValue()
    }
}

impl Reflectable for Window {
    fn reflector<'a>(&'a self) -> &'a Reflector {
        self.eventtarget.reflector()
    }

    fn mut_reflector<'a>(&'a mut self) -> &'a mut Reflector {
        self.eventtarget.mut_reflector()
    }
}

impl Window {
    fn set_timeout_or_interval(&mut self, callback: JSVal, timeout: i32, is_interval: bool) -> i32 {
        let timeout = cmp::max(0, timeout) as u64;
        let handle = self.next_timer_handle;
        self.next_timer_handle += 1;

        // Post a delayed message to the per-window timer task; it will dispatch it
        // to the relevant script handler that will deal with it.
        let tm = Timer::new().unwrap();
        let (cancel_chan, cancel_port) = channel();
        let chan = self.timer_chan.clone();
        let spawn_name = if is_interval {
            "Window:SetInterval"
        } else {
            "Window:SetTimeout"
        };
        spawn_named(spawn_name, proc() {
            let mut tm = tm;
            let timeout_port = if is_interval {
                tm.periodic(timeout)
            } else {
                tm.oneshot(timeout)
            };
            let cancel_port = cancel_port;

            let select = Select::new();
            let mut timeout_handle = select.handle(&timeout_port);
            unsafe { timeout_handle.add() };
            let mut cancel_handle = select.handle(&cancel_port);
            unsafe { cancel_handle.add() };

            loop {
                let id = select.wait();
                if id == timeout_handle.id() {
                    timeout_port.recv();
                    chan.send(TimerMessageFire(~TimerData {
                        handle: handle,
                        is_interval: is_interval,
                        funval: callback,
                        args: ~[],
                    }));
                    if !is_interval {
                        break;
                    }
                } else if id == cancel_handle.id() {
                    break;
                }
            }
        });
        self.active_timers.insert(handle, TimerHandle { handle: handle, cancel_chan: Some(cancel_chan) });
        handle
    }

    pub fn SetTimeout(&mut self, _cx: *JSContext, callback: JSVal, timeout: i32) -> i32 {
        self.set_timeout_or_interval(callback, timeout, false)
    }

    pub fn ClearTimeout(&mut self, handle: i32) {
        let timer_handle = self.active_timers.pop(&handle);
        match timer_handle {
            Some(handle) => handle.cancel(),
            None => { }
        }
    }

    pub fn SetInterval(&mut self, _cx: *JSContext, callback: JSVal, timeout: i32) -> i32 {
        self.set_timeout_or_interval(callback, timeout, true)
    }

    pub fn ClearInterval(&mut self, handle: i32) {
        self.ClearTimeout(handle);
    }

    pub fn damage_and_reflow(&self, damage: DocumentDamageLevel) {
        // FIXME This should probably be ReflowForQuery, not Display. All queries currently
        // currently rely on the display list, which means we can't destroy it by
        // doing a query reflow.
        self.page().damage(damage);
        self.page().reflow(ReflowForDisplay, self.script_chan.clone(), *self.compositor);
    }

    pub fn wait_until_safe_to_modify_dom(&self) {
        // FIXME: This disables concurrent layout while we are modifying the DOM, since
        //        our current architecture is entirely unsafe in the presence of races.
        self.page().join_layout();
    }

    pub fn new(cx: *JSContext,
               page: Rc<Page>,
               script_chan: ScriptChan,
               compositor: ~ScriptListener,
               image_cache_task: ImageCacheTask)
               -> JS<Window> {
        let script_chan_clone = script_chan.clone();
        let (timer_chan, timer_port): (Sender<TimerControlMsg>, Receiver<TimerControlMsg>) = channel();
        let id = page.id.clone();
        spawn_named("timer controller", proc() {
            let ScriptChan(script_chan) = script_chan;
            loop {
                match timer_port.recv() {
                    TimerMessageClose => break,
                    TimerMessageFire(td) => script_chan.send(FireTimerMsg(id, td)),
                    TimerMessageTriggerExit => script_chan.send(ExitWindowMsg(id)),
                }
            }
        });

        let win = ~Window {
            eventtarget: EventTarget::new_inherited(WindowTypeId),
            script_chan: script_chan_clone,
            console: None,
            compositor: Untraceable::new(compositor),
            timer_chan: Untraceable::new(timer_chan),
            page: page.clone(),
            location: None,
            navigator: None,
            image_cache_task: image_cache_task,
            active_timers: ~HashMap::new(),
            next_timer_handle: 0
        };

        let global = WindowBinding::Wrap(cx, win);
        let fn_names = ["window", "self"];
        for str in fn_names.iter() {
            (*str).to_c_str().with_ref(|name| {
                let object = global.reflector().get_jsobject();
                assert!(object.is_not_null());
                unsafe {
                    JS_DefineProperty(cx, object, name,
                                      ObjectValue(&*object),
                                      Some(JS_PropertyStub),
                                      Some(JS_StrictPropertyStub),
                                      JSPROP_ENUMERATE);
                }
            })

        }
        global
    }
}
