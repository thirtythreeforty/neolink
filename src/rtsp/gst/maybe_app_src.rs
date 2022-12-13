use super::*;
use crossbeam_channel::{bounded, Receiver, Sender};

/// A Write implementation around AppSrc that also allows delaying the creation of the AppSrc
/// until later, discarding written data until the AppSrc is provided.
pub(crate) struct MaybeAppSrc {
    rx: Receiver<AppSrc>,
    app_src: Option<AppSrc>,
}

impl MaybeAppSrc {
    /// Creates a MaybeAppSrc.  Also returns a Sender that you must use to provide an AppSrc as
    /// soon as one is available.  When it is received, the MaybeAppSrc will start pushing data
    /// into the AppSrc when write() is called.
    pub(crate) fn new_with_tx() -> (Self, Sender<AppSrc>) {
        let (tx, rx) = bounded(3); // The sender should not send very often
        (MaybeAppSrc { rx, app_src: None }, tx)
    }

    /// Calls end of stream to Gstreamer, using during drop
    pub(crate) fn end_of_stream(&mut self) {
        if let Some(src) = self.try_get_src() {
            // Ignore "errors" from Gstreamer such as FLUSHING, which are not really errors.
            let _ = src.end_of_stream();
        }
    }

    /// Attempts to retrieve the AppSrc that should be passed in by the caller of new_with_tx
    /// at some point after this struct has been created.  At that point, we swap over to
    /// owning the AppSrc directly.  This function handles either case and returns the AppSrc,
    /// or None if the caller has not yet sent one.
    fn try_get_src(&mut self) -> Option<&AppSrc> {
        while let Ok(src) = self.rx.try_recv() {
            self.app_src = Some(src);
        }
        self.app_src.as_ref()
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.app_src.is_some()
    }
}

impl Write for MaybeAppSrc {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // If we have no AppSrc yet, throw away the data and claim that it was written
        let app_src = match self.try_get_src() {
            Some(src) => src,
            None => return Ok(buf.len()),
        };

        let mut gst_buf = gstreamer::Buffer::with_size(buf.len()).unwrap();
        {
            let gst_buf_mut = gst_buf.get_mut().unwrap();
            let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
            gst_buf_data.copy_from_slice(buf);
        }

        let res = app_src.push_buffer(gst_buf); //.map_err(|e| io::Error::new(io::ErrorKind::Other, Box::new(e)))?;
        if res.is_err() {
            self.app_src = None;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for MaybeAppSrc {
    fn drop(&mut self) {
        self.end_of_stream();
    }
}
