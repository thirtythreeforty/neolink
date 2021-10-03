use anyhow::{anyhow, Error as Anyhow};
use gstreamer::prelude::ElementExt;
use gstreamer::prelude::ObjectExt;
use gstreamer::{Element, Pad};
use log::*;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

pub(crate) struct MaybeInputSelect {
    rx: Receiver<Element>,
    typefind: Option<Element>,
}

impl MaybeInputSelect {
    pub(crate) fn new_with_tx() -> (Self, SyncSender<Element>) {
        let (tx, rx) = sync_channel(3); // The sender should not send very often
        (MaybeInputSelect { rx, typefind: None }, tx)
    }

    pub(crate) fn try_get_src(&mut self) -> Option<&Element> {
        while let Some(src) = self.rx.try_recv().ok() {
            self.typefind = Some(src);
        }
        self.typefind.as_ref()
    }

    pub(crate) fn set_input(&mut self, path_num: u32) -> Result<(), Anyhow> {
        if let Some(element) = self.try_get_src() {
            let new_pad = element
                .static_pad(&format!("sink_{}", path_num))
                .ok_or_else(|| anyhow!("Unable to set input pad"))?;

            if let Ok(Ok(active_pad)) = element.property("active-pad").map(|e| e.get::<'_, Pad>()) {
                if active_pad != new_pad {
                    debug!("Pad need changing");
                    if let Err(e) = element.set_property("active-pad", new_pad) {
                        debug!("Element is invalid: {:?}", e);
                        self.clear();
                    } else {
                        debug!("Pad chanaged to {}", path_num);
                    }
                } else {
                    debug!("Pad already set to {}", path_num);
                }
            } else {
                debug!("Element is invalid");
                self.clear();
            }
        }
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.typefind = None;
    }
}
