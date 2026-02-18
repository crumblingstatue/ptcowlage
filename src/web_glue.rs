use ptcow::VoiceIdx;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::app::ui::file_ops::FileOp;

#[wasm_bindgen(module = "/web_glue.js")]
extern "C" {
    fn open_file_dialog(accept: &str) -> js_sys::Promise;
}

#[wasm_bindgen(module = "/web_glue.js")]
unsafe extern "C" {
    pub fn save_file(data: &[u8], filename: &str);
}

pub fn request_fullscreen() {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let element = document.document_element().unwrap();

    let _ = element.request_fullscreen();
}

pub fn exit_fullscreen() {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let _ = document.exit_fullscreen();
}

pub struct OpenedFile {
    pub name: String,
    pub data: Vec<u8>,
}

pub async fn open_file(accept: &str) -> OpenedFile {
    let js_value = JsFuture::from(open_file_dialog(accept)).await.unwrap();
    let array = js_sys::Array::from(&js_value);
    let name = array.get(0).as_string().unwrap();
    let data = js_sys::Uint8Array::new(&array.get(1)).to_vec();
    OpenedFile { name, data }
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
    OpenFile {
        data: Vec<u8>,
        name: String,
    },
    ImportMidi {
        data: Vec<u8>,
    },
    ImportPiyo {
        data: Vec<u8>,
    },
    ImportOrganya {
        data: Vec<u8>,
    },
    ImportPtVoice {
        data: Vec<u8>,
        name: String,
    },
    ImportPtNoise {
        data: Vec<u8>,
        name: String,
    },
    ReplaceVoicesPtCop {
        data: Vec<u8>,
    },
    ReplacePtVoiceSingle {
        data: Vec<u8>,
        name: String,
        voice_idx: VoiceIdx,
    },
    ReplacePtNoiseSingle {
        data: Vec<u8>,
        name: String,
        voice_idx: VoiceIdx,
    },
}

impl WebCmd {
    pub fn from_file_op(file_op: FileOp, data: Vec<u8>, name: String) -> Self {
        match file_op {
            FileOp::OpenProj => Self::OpenFile { data, name },
            FileOp::ReplaceVoicesPtcop => Self::ReplaceVoicesPtCop { data },
            FileOp::ImportMidi => Self::ImportMidi { data },
            FileOp::SaveProjAs => todo!(),
            FileOp::ImportPiyoPiyo => Self::ImportPiyo { data },
            FileOp::ImportOrganya => Self::ImportOrganya { data },
            FileOp::ExportWav => todo!(),
            FileOp::ReplaceSf2Single(voice_idx) => todo!(),
            FileOp::ImportSf2Single => todo!(),
            FileOp::ImportPtNoise => Self::ImportPtNoise { data, name },
            FileOp::ImportPtVoice => Self::ImportPtVoice { data, name },
            FileOp::ReplacePtVoiceSingle(voice_idx) => Self::ReplacePtVoiceSingle {
                data,
                name,
                voice_idx,
            },
            FileOp::ReplacePtNoiseSingle(voice_idx) => Self::ReplacePtNoiseSingle {
                data,
                name,
                voice_idx,
            },
        }
    }
}
