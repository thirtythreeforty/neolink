use gstreamer::Element;
use crossbeam_channel::{sync_channel, Receiver, SyncSender};

pub(crate) struct MaybeOutputSelect {
    rx: Receiver<Element>,
    typefind: Option<Element>,
}

impl MaybeOutputSelect {
    pub(crate) fn new_with_tx() -> (Self, SyncSender<Element>) {
        let (tx, rx) = sync_channel(3); // The sender should not send very often
        (MaybeOutputSelect { rx, typefind: None }, tx)
    }

    pub(crate) fn try_get_src(&mut self) -> Option<&Element> {
        while let Some(src) = self.rx.try_recv().ok() {
            self.typefind = Some(src);
        }
        self.typefind.as_ref()
    }

    pub(crate) fn clear(&mut self) {
        self.typefind = None;
    }
}
