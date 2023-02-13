use anyhow::{anyhow, Error as Anyhow};
use crossbeam_channel::{bounded, Receiver, Sender};
use gstreamer::prelude::ElementExt;
use gstreamer::prelude::ObjectExt;
use gstreamer::{Element, Pad};
use log::*;

pub(crate) struct MaybeInputSelect {
    rx: Receiver<Element>,
    typefind: Option<Element>,
}

impl MaybeInputSelect {
    pub(crate) fn new_with_tx() -> (Self, Sender<Element>) {
        let (tx, rx) = bounded(3); // The sender should not send very often
        (MaybeInputSelect { rx, typefind: None }, tx)
    }

    pub(crate) fn try_get_src(&mut self) -> Option<&Element> {
        while let Ok(src) = self.rx.try_recv() {
            self.typefind = Some(src);
        }
        self.typefind.as_ref()
    }

    pub(crate) fn set_input(&mut self, path_num: u32) -> Result<(), Anyhow> {
        if let Some(element) = self.try_get_src() {
            let new_pad = element
                .static_pad(&format!("sink_{}", path_num))
                .ok_or_else(|| anyhow!("Unable to set input pad"))?;

            match element.property_value("active-pad").get::<'_, Pad>() {
                Ok(active_pad) => {
                    if active_pad != new_pad {
                        debug!("Pad need changing");
                        element.set_property("active-pad", new_pad);
                    }
                }
                Err(e) => {
                    debug!("Pad need changing: Active pad invalid: {:?}", e);
                    element.set_property("active-pad", new_pad);
                }
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.typefind = None;
    }
}
