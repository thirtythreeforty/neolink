use anyhow::{anyhow, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use gstreamer::{
    element_error, parse_launch, prelude::*, Caps, ClockTime, FlowError, FlowSuccess, MessageView,
    Pipeline, ResourceError, State,
};
use gstreamer_app::{AppSink, AppSinkCallbacks};

use byte_slice_cast::*;

pub(super) fn from_input(
    input_src: &str,
    volume: f32,
    block_align: u16,
    sample_rate: u16,
) -> Result<Receiver<Vec<u8>>> {
    let pipeline = create_pipeline(input_src, volume, block_align, sample_rate)?;
    input(pipeline)
}

fn input(pipeline: Pipeline) -> Result<Receiver<Vec<u8>>> {
    let appsink = get_sink(&pipeline)?;
    let (tx, rx) = bounded(30);

    set_data_channel(&appsink, tx);

    std::thread::spawn(move || {
        let _ = start_pipeline(pipeline);
    });

    Ok(rx)
}

fn start_pipeline(pipeline: Pipeline) -> Result<()> {
    pipeline.set_state(State::Playing)?;

    let bus = pipeline
        .bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    for msg in bus.iter_timed(ClockTime::NONE) {
        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline
                    .set_state(State::Null)
                    .context("Error in gstreamer when setting state to Null")?;
                log::warn!(
                    "Error from gstreamer when setting the play state {:?} setting to Null instead",
                    err
                );
            }
            _ => (),
        }
    }

    pipeline
        .set_state(State::Null)
        .context("Error in gstreamer when setting state to Null")?;

    Ok(())
}

fn get_sink(pipeline: &Pipeline) -> Result<AppSink> {
    let sink = pipeline
        .by_name("thesink")
        .expect("There shoud be a `thesink`");
    sink.dynamic_cast::<AppSink>()
        .map_err(|_| anyhow!("Cannot find appsink in gstreamer, check your gstreamer plugins"))
}

fn set_data_channel(appsink: &AppSink, tx: Sender<Vec<u8>>) {
    // Getting data out of the appsink is done by setting callbacks on it.
    // The appsink will then call those handlers, as soon as data is available.
    appsink.set_callbacks(
        AppSinkCallbacks::builder()
            // Add a handler to the "new-sample" signal.
            .new_sample(move |appsink| {
                // Pull the sample in question out of the appsink's buffer.
                let sample = appsink.pull_sample().map_err(|_| FlowError::Eos)?;
                let buffer = sample.buffer().ok_or_else(|| {
                    element_error!(
                        appsink,
                        ResourceError::Failed,
                        ("Failed to get buffer from appsink")
                    );

                    FlowError::Error
                })?;

                // At this point, buffer is only a reference to an existing memory region somewhere.
                // When we want to access its content, we have to map it while requesting the required
                // mode of access (read, read/write).
                // This type of abstraction is necessary, because the buffer in question might not be
                // on the machine's main memory itself, but rather in the GPU's memory.
                // So mapping the buffer makes the underlying memory region accessible to us.
                // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
                let map = buffer.map_readable().map_err(|_| {
                    element_error!(
                        appsink,
                        ResourceError::Failed,
                        ("Failed to map buffer readable")
                    );

                    FlowError::Error
                })?;

                // We know what format the data in the memory region has, since we requested
                // it by setting the appsink's caps. So what we do here is interpret the
                // memory region we mapped as an array of signed 8 bit integers.
                let samples = map.as_slice_of::<u8>().map_err(|_| {
                    element_error!(
                        appsink,
                        ResourceError::Failed,
                        ("Failed to interprete buffer as u8 ADPCM")
                    );

                    FlowError::Error
                })?;

                // Ready!
                let _ = tx.send(samples.to_vec());

                Ok(FlowSuccess::Ok)
            })
            .build(),
    );
}

fn create_pipeline(
    source: &str,
    volume: f32,
    block_align: u16,
    sample_rate: u16,
) -> Result<Pipeline> {
    gstreamer::init()
        .context("Unable to start gstreamer ensure it and all plugins are installed")?;

    let launch_str = format!(
        "{} \
        ! decodebin \
        ! audioconvert \
        ! audioresample \
        ! audio/x-raw,rate={},channels=1 \
        ! volume volume={:.2} \
        ! queue  \
        ! adpcmenc blockalign={} layout=dvi \
        ! appsink name=thesink",
        source, sample_rate, volume, block_align
    );

    log::info!("{}", launch_str);

    // Parse the pipeline we want to probe from a static in-line string.
    // Here we give our audiotestsrc a name, so we can retrieve that element
    // from the resulting pipeline.
    let pipeline = parse_launch(&launch_str)
        .context("Unable to load gstreamer pipeline ensure all gstramer plugins are installed")?;
    let pipeline = pipeline.dynamic_cast::<Pipeline>().map_err(|_| {
        anyhow!("Unable to create gstreamer pipeline ensure all gstramer plugins are installed")
    })?;

    let appsink = get_sink(&pipeline)?;

    // Tell the appsink what format we want. It will then be the audiotestsrc's job to
    // provide the format we request.
    // This can be set after linking the two objects, because format negotiation between
    // both elements will happen during pre-rolling of the pipeline.
    appsink.set_caps(Some(&Caps::new_simple(
        "audio/x-adpcm",
        &[
            ("layout", &"dvi"),
            ("block_align", &(block_align as i32)),
            ("channels", &(1i32)),
            ("rate", &(sample_rate as i32)),
        ],
    )));

    Ok(pipeline)
}
