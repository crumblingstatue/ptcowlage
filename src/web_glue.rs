use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/web_glue.js")]
extern "C" {
    fn open_file_dialog() -> js_sys::Promise;
}

#[wasm_bindgen(module = "/web_glue.js")]
unsafe extern "C" {
    pub fn save_file(data: &[u8], filename: &str);
}

pub async fn open_file() -> Vec<u8> {
    let js_value = JsFuture::from(open_file_dialog()).await.unwrap();
    let array = js_sys::Uint8Array::new(&js_value);
    array.to_vec()
}

/// Web command queue that needs to communicate through `'static` boundaries
#[derive(Default)]
pub struct WebCmdQueue {
    queue: std::collections::VecDeque<WebCmd>,
}

impl WebCmdQueue {
    pub fn pop(&mut self) -> Option<WebCmd> {
        self.queue.pop_front()
    }
}

pub type WebCmdQueueHandle = Rc<RefCell<WebCmdQueue>>;

pub trait WebCmdQueueHandleExt {
    fn push(&self, cmd: WebCmd);
}

impl WebCmdQueueHandleExt for WebCmdQueueHandle {
    fn push(&self, cmd: WebCmd) {
        self.borrow_mut().queue.push_back(cmd);
    }
}

pub enum WebCmd {
    OpenFile { data: Vec<u8> },
    ImportMidi { data: Vec<u8> },
    ImportPiyo { data: Vec<u8> },
    ImportOrganya { data: Vec<u8> },
}
