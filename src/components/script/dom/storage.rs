use dom::bindings::codegen::StorageBinding;
use dom::bindings::utils::{Reflector, Reflectable, reflect_dom_object};
use dom::bindings::js::JS;
use dom::window::Window;
use servo_util::str::DOMString;

#[deriving(Encodable)]
pub struct Storage {
    reflector_: Reflector,
}

impl Storage {
    pub fn new_inherited() -> Storage {
        Storage {
            reflector_: Reflector::new()
        }
    }

    pub fn new(window: &JS<Window>) -> JS<Storage> {
        reflect_dom_object(~Storage::new_inherited(), window, StorageBinding::Wrap)
    }

    pub fn Length(&self) -> u32 { 0 }
    pub fn Key(&self, index: u32) -> Option<DOMString> { None }
    pub fn GetItem(&self, key: DOMString) -> Option<DOMString> { None }
    pub fn SetItem(&self, key: DOMString, value: DOMString) { }
    pub fn Clear(&self) { }

    pub fn NamedGetter(&self, key: Option<DOMString>, found: &mut bool) -> Option<DOMString> { None }
    pub fn NamedSetter(&self, key: Option<DOMString>, value: DOMString) { }
}

impl Reflectable for Storage {
    fn reflector<'a>(&'a self) -> &'a Reflector {
        &self.reflector_
    }

    fn mut_reflector<'a>(&'a mut self) -> &'a mut Reflector {
        &mut self.reflector_
    }
}
