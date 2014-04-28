use collections::HashMap;

use dom::bindings::codegen::StorageBinding;
use dom::bindings::utils::{Reflector, Reflectable, reflect_dom_object};
use dom::bindings::js::JS;
use dom::window::Window;
use servo_util::str::DOMString;

#[deriving(Encodable)]
pub struct Storage {
    reflector_: Reflector,
    store: HashMap<DOMString, DOMString>,
}

impl Storage {
    pub fn new_inherited() -> Storage {
        Storage {
            reflector_: Reflector::new(),
            store: HashMap::new(),
        }
    }

    pub fn new(window: &JS<Window>) -> JS<Storage> {
        reflect_dom_object(~Storage::new_inherited(), window, StorageBinding::Wrap)
    }

    pub fn Length(&self) -> u32 {
        self.store.len() as u32
    }

    pub fn Key(&self, index: u32) -> Option<DOMString> {
        self.store.keys().nth(index as uint).map(|s| s.to_owned())
    }

    pub fn GetItem(&self, key: DOMString) -> Option<DOMString> {
        self.store.find_copy(&key)
    }

    pub fn SetItem(&mut self, key: DOMString, value: DOMString) {
        self.store.insert(key, value);
    }

    pub fn Clear(&mut self) {
        self.store.clear();
    }

    pub fn NamedGetter(&self, maybe_key: Option<DOMString>, found: &mut bool) -> Option<DOMString> {
        match maybe_key {
            Some(key) => {
                let maybe_value = self.GetItem(key);
                *found = maybe_value.is_some();
                maybe_value
            },
            None => {
                *found = false;
                None
            }
        }
    }

    pub fn NamedSetter(&mut self, maybe_key: Option<DOMString>, value: DOMString) {
        maybe_key.map(|key| self.SetItem(key, value.clone()));
    }
}

impl Reflectable for Storage {
    fn reflector<'a>(&'a self) -> &'a Reflector {
        &self.reflector_
    }

    fn mut_reflector<'a>(&'a mut self) -> &'a mut Reflector {
        &mut self.reflector_
    }
}
