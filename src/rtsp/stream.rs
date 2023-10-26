use anyhow::{anyhow, Result};
use gstreamer::{prelude::*, ClockTime, FlowError};
use gstreamer_app::AppSrc;
use gstreamer_rtsp_server::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::{
    sync::{broadcast::channel as broadcast, watch::channel as watch},
    task::JoinSet,
    time::{sleep, sleep_until, Duration, Instant},
};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::common::{Permit, StampedData, UseCounter};
use crate::{
    common::{NeoInstance, StreamConfig, StreamInstance},
    AnyResult,
};

use super::{factory::*, gst::NeoRtspServer};

#[derive(Clone)]
struct PauseAffectors {
    motion: bool,
    push: bool,
    client: bool,
}

/// This handles the stream by activating and deacivating it as required
pub(super) async fn stream_main(
    mut stream_instance: StreamInstance,
    camera: NeoInstance,
    rtsp: &NeoRtspServer,
    users: &HashSet<String>,
    paths: &[String],
) -> Result<()> {
    let mut camera_config = camera.config().await?.clone();
    let name = camera_config.borrow().name.clone();

    let mut curr_pause;
    loop {
        let this_loop_cancel = CancellationToken::new();
        let _drop_guard = this_loop_cancel.clone().drop_guard();

        log::debug!("{}: Activating Stream", &name);
        stream_instance.activate().await?;

        // Wait for a valid stream format to be detected
        log::debug!("{}: Waiting for Valid Stream", &name);
        stream_instance
            .config
            .wait_for(|config| {
                log::debug!("{:?}", config);
                config.vid_ready()
            })
            .await?;
        log::debug!("{}: Waiting for Valid Audio", &name);
        // After vid give it 1s to look for audio
        // Ignore timeout but check err
        if let Ok(v) = tokio::time::timeout(
            Duration::from_secs(1),
            stream_instance.config.wait_for(|config| {
                log::debug!("{:?}", config);
                config.aud_ready()
            }),
        )
        .await
        {
            v?;
        }

        curr_pause = camera_config.borrow().pause.clone();

        let last_stream_config = stream_instance.config.borrow().clone();
        let mut thread_stream_config = stream_instance.config.clone();

        let (pause_affector_tx, pause_affector) = watch(PauseAffectors {
            motion: false,
            push: false,
            client: false,
        });
        let pause_affector_tx = Arc::new(pause_affector_tx);

        let mut set = JoinSet::<AnyResult<()>>::new();
        log::debug!("{}: Creating Client Counters", &name);
        // Handles the on off of the stream with the client pause
        let client_counter = UseCounter::new().await;
        let client_count = client_counter.create_deactivated().await?;

        // Client count affector
        if curr_pause.on_motion {
            let thread_name = name.clone();
            let client_count = client_counter.create_deactivated().await?;
            let thread_pause_affector_tx = pause_affector_tx.clone();
            let cancel = this_loop_cancel.clone();
            set.spawn(async move {
                tokio::select! {
                    _ = cancel.cancelled() => AnyResult::Ok(()),
                    v = async {
                        log::debug!("{}: Activating Client Pause", thread_name);
                        loop {
                            client_count.aquired_users().await?;
                            log::info!("{}: Enabling Client", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.client = true;
                            });

                            client_count.dropped_users().await?;
                            log::info!("{}: Pausing Client", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.client = false;
                            });
                        }
                    } => v,
                }
            });
        }

        // Motion affector
        if curr_pause.on_motion {
            let thread_name = name.clone();
            let thread_pause_affector_tx = pause_affector_tx.clone();
            let cancel = this_loop_cancel.clone();

            let mut motion = camera.motion().await?;
            let delta = Duration::from_secs_f64(curr_pause.motion_timeout);

            set.spawn(async move {
                tokio::select! {
                    _ = cancel.cancelled() => AnyResult::Ok(()),
                    v = async {
                        log::debug!("{}: Activating Motion Pause", &thread_name);
                        loop {
                            motion
                                .wait_for(|md| matches!(md, crate::common::MdState::Start(_)))
                                .await?;
                            log::info!("{}: Enabling Motion", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.motion = true;
                            });

                            motion
                                .wait_for(
                                    |md| matches!(md, crate::common::MdState::Stop(n) if n.elapsed()>delta),
                                )
                                .await?;
                            log::info!("{}: Pausing Motion", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.motion = false;
                            });
                        }
                    } => v,
                }
            });

            // Push notfications
            log::debug!("{}: Activating Push Notification Pause", &name);
            let mut pn = camera.push_notifications().await?;
            let mut curr_pn = None;
            let thread_name = name.clone();
            let thread_pause_affector_tx = pause_affector_tx.clone();
            let cancel = this_loop_cancel.clone();
            set.spawn(async move {
                tokio::select! {
                    _ = cancel.cancelled() => AnyResult::Ok(()),
                    v = async {
                        loop {
                            curr_pn = pn
                                .wait_for(|pn| pn != &curr_pn && pn.is_some())
                                .await?
                                .clone();
                            log::info!("{}: Enabling Push Notification", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.push = true;
                            });
                            tokio::select! {
                                v = pn.wait_for(|pn| pn != &curr_pn && pn.is_some()) => {
                                    v?;
                                    // If another PN during wait then go back to wait more
                                    continue;
                                }
                                _ = sleep(Duration::from_secs(30)) => {}
                            }
                            log::info!("{}: Pausing Push Notification", thread_name);
                            thread_pause_affector_tx.send_modify(|current| {
                                current.push = false;
                            });
                        }
                    } => v,
                }
            });
        }

        if curr_pause.on_motion || curr_pause.on_disconnect {
            // Take over activation
            let cancel = this_loop_cancel.clone();
            let mut client_activator = stream_instance.activator_handle().await;
            client_activator.deactivate().await?;
            stream_instance.deactivate().await?;
            let mut pause_affector = tokio_stream::wrappers::WatchStream::new(pause_affector);
            let thread_curr_pause = curr_pause.clone();
            set.spawn(async move {
                tokio::select! {
                    _ = cancel.cancelled() => AnyResult::Ok(()),
                    v = async {
                        while let Some(state) = pause_affector.next().await {
                            if thread_curr_pause.on_motion && thread_curr_pause.on_disconnect {
                                if state.client && (state.motion || state.push) {
                                    client_activator.activate().await?;
                                } else {
                                    client_activator.deactivate().await?;
                                }
                            } else if thread_curr_pause.on_motion {
                                if state.motion || state.push {
                                    client_activator.activate().await?;
                                } else {
                                    client_activator.deactivate().await?;
                                }
                            } else if thread_curr_pause.on_disconnect {
                                if state.client {
                                    client_activator.activate().await?;
                                } else {
                                    client_activator.deactivate().await?;
                                }
                            } else {
                                unreachable!()
                            }
                        }
                        AnyResult::Ok(())
                    } => v,
                }
            });
        }

        // This thread jsut keeps it active for 5s after an initial start to build the buffer
        let cancel = this_loop_cancel.clone();
        let mut init_activator = stream_instance.activator_handle().await;
        let init_camera = camera.clone();
        set.spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    init_activator.activate().await?;
                    let _ = init_camera
                        .run_task(|_| {
                            Box::pin(async move {
                                sleep(Duration::from_secs(5)).await;
                                AnyResult::Ok(())
                            })
                        })
                        .await;
                    init_activator.deactivate().await?;
                    AnyResult::Ok(())
                } => v,
            }
        });

        // Task to just report the number of clients for debug purposes
        let cancel = this_loop_cancel.clone();
        let counter = client_counter.create_deactivated().await?;
        let mut cur_count = 0;
        let thread_name = name.clone();
        set.spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    loop {
                        cur_count = *counter.get_counter().wait_for(|v| v != &cur_count).await?;
                        log::debug!("{thread_name}: Number of rtsp clients: {cur_count}");
                    }
                } => v,
            }
        });

        // This runs the actual stream.
        // The select will restart if the stream's config updates
        log::debug!("{}: Stream Activated", &name);
        break tokio::select! {
            v = thread_stream_config.wait_for(|new_conf| new_conf != &last_stream_config) => {
                let v = v?;
                // If stream config changes we reload the stream
                log::info!("{}: Stream Configuration Changed. Reloading Streams", &name);
                log::trace!("    From {:?} to {:?}", last_stream_config, v.clone());
                continue;
            },
            v = camera_config.wait_for(|new_conf| new_conf.pause != curr_pause ) => {
                v?;
                // If pause config changes restart
                log::info!("{}: Pause Configuration Changed. Reloading Streams", &name);
                continue;
            },
            v = stream_run(&name, &stream_instance, rtsp, &last_stream_config, users, paths, client_count) => v,
        };
    }
}

/// This handles the stream itself by creating the factory and pushing messages into it
async fn stream_run(
    name: &str,
    stream_instance: &StreamInstance,
    rtsp: &NeoRtspServer,
    stream_config: &StreamConfig,
    users: &HashSet<String>,
    paths: &[String],
    client_count: Permit,
) -> AnyResult<()> {
    let vidstream = stream_instance.vid.resubscribe();
    let audstream = stream_instance.aud.resubscribe();
    let vid_history = stream_instance.vid_history.clone();
    let aud_history = stream_instance.aud_history.clone();

    // Finally ready to create the factory and connect the stream
    let mounts = rtsp
        .mount_points()
        .ok_or(anyhow!("RTSP server lacks mount point"))?;
    // Create the factory
    let (factory, mut client_rx) = make_factory(stream_config).await?;

    factory.add_permitted_roles(users);

    for path in paths.iter() {
        log::debug!("Path: {}", path);
        mounts.add_factory(path, factory.clone());
    }
    log::info!("{}: Avaliable at {}", name, paths.join(", "));

    let stream_cancel = CancellationToken::new();
    let drop_guard = stream_cancel.clone().drop_guard();
    let mut set = JoinSet::new();
    // Wait for new media client data to come in from the factory
    while let Some(mut client_data) = client_rx.recv().await {
        log::debug!("New media");
        // New media created
        let vid = client_data.vid.take().map(|data| data.app);
        let aud = client_data.aud.take().map(|data| data.app);

        // This is the data that gets sent to gstreamer thread
        // It represents the combination of the camera stream and the appsrc seek messages
        // At 30fps for 15s with audio you need about 900 frames
        // Therefore the buffer is rather large at 2000
        let (aud_data_tx, aud_data_rx) = broadcast(2000);
        let (vid_data_tx, vid_data_rx) = broadcast(2000);

        // This thread takes the video data from the cam and passed it into the stream
        let mut vidstream = BroadcastStream::new(vidstream.resubscribe());
        let thread_vid_data_tx = vid_data_tx.clone();
        let thread_stream_cancel = stream_cancel.clone();
        let thread_vid_history = vid_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    // Send Initial
                    {
                        let history = thread_vid_history.borrow();
                        // let last_ts = history.back().map(|s| s.ts);
                        for data in history.iter() {
                            thread_vid_data_tx.send(
                                // StampedData {
                                //     keyframe: data.keyframe,
                                //     data: data.data.clone(),
                                //     ts: last_ts.unwrap()
                                // }
                                data.clone()
                            )?;
                        }
                    }

                    // Send new
                    while let Some(frame) = vidstream.next().await {
                        if let Ok(data) = frame {
                            thread_vid_data_tx.send(
                                data
                            )?;
                        }
                    };
                    AnyResult::Ok(())
                } => v,
            };
            log::trace!("Stream Vid Media End {r:?}");
            AnyResult::Ok(())
        });

        // This thread takes the audio data from the cam and passed it into the stream
        let mut audstream = BroadcastStream::new(audstream.resubscribe());
        let thread_stream_cancel = stream_cancel.clone();
        let thread_aud_data_tx = aud_data_tx.clone();
        let thread_aud_history = aud_history.clone();
        set.spawn(async move {
            let r = tokio::select! {
                _ = thread_stream_cancel.cancelled() => AnyResult::Ok(()),
                v = async {
                    // Send Initial
                    {
                        let history = thread_aud_history.borrow();
                        // let last_ts = history.back().map(|s| s.ts);
                        for data in history.iter() {
                            thread_aud_data_tx.send(
                                // StampedData {
                                //     keyframe: data.keyframe,
                                //     data: data.data.clone(),
                                //     ts: last_ts.unwrap()
                                // }
                                data.clone()

                            )?;
                        }
                    }

                    // Send new
                    while let Some(frame) = audstream.next().await {
                        if let Ok(data) = frame {
                            thread_aud_data_tx.send(
                                data
                            )?;
                        }
                    };
                    AnyResult::Ok(())
                } => v,
            };
            log::trace!("Stream Aud Media End: {r:?}");
            AnyResult::Ok(())
        });

        // Handles sending the video data into gstreamer
        let thread_stream_cancel = stream_cancel.clone();
        let vid_data_rx = BroadcastStream::new(vid_data_rx).filter(|f| f.is_ok()); // Filter to ignore lagged
        let thread_vid = vid.clone();
        let mut thread_client_count = client_count.subscribe();
        log::debug!("stream_config.fps: {}", stream_config.fps);
        // let fallback_time = Duration::from_secs(3);
        // let fallback_framerate =
        //     Duration::from_millis(1000u64 / std::cmp::max(stream_config.fps as u64, 5u64));
        if let Some(thread_vid) = thread_vid {
            set.spawn(async move {
                thread_client_count.activate().await?;
                let r = tokio::select! {
                    _ = thread_stream_cancel.cancelled() => {
                        AnyResult::Ok(())
                    },
                    v = send_to_appsrc(
                        repeat_keyframe(
                            frametime_stream(
                                hold_stream(
                                    wait_for_keyframe(
                                        vid_data_rx,
                                    )
                                )
                            ),
                            fallback_time,
                            fallback_framerate,
                        ),
                        &thread_vid) => {
                        v
                    },
                };
                drop(thread_client_count);
                let _ = thread_vid.end_of_stream();
                log::debug!("Vid Thread End: {:?}", r);
                r
            });
        }

        // Handles the audio data into gstreamer
        let thread_stream_cancel = stream_cancel.clone();
        let aud_data_rx = BroadcastStream::new(aud_data_rx).filter(|f| f.is_ok()); // Filter to ignore lagged
        let thread_aud = aud.clone();
        if let Some(thread_aud) = thread_aud {
            set.spawn(async move {
                let r = tokio::select! {
                    _ = thread_stream_cancel.cancelled() => {
                        AnyResult::Ok(())
                    },
                    v = send_to_appsrc(
                        frametime_stream(
                            hold_stream(
                                wait_for_keyframe(
                                    aud_data_rx
                                )
                            )
                        ), &thread_aud) => {
                        v
                    },
                };
                let _ = thread_aud.end_of_stream();
                log::debug!("Aud Thread End: {:?}", r);
                r
            });
        }
    }
    log::debug!("Cleaning up streams");
    // At this point the factory has been destroyed
    // Cancel any remaining threads that are trying to send data
    // Although it should be finished already when the appsrcs are dropped
    stream_cancel.cancel();
    drop(drop_guard);
    while set.join_next().await.is_some() {}
    log::trace!("Stream done");
    AnyResult::Ok(())
}

fn check_live(app: &AppSrc) -> Result<()> {
    // log::debug!("Checking Live: {:?}", app.bus());
    app.bus().ok_or(anyhow!("App source is closed"))?;
    app.pads()
        .iter()
        .all(|pad| pad.is_linked())
        .then_some(())
        .ok_or(anyhow!("App source is closed"))
}

fn get_runtime(app: &AppSrc) -> Option<Duration> {
    if let Some(clock) = app.clock() {
        if let Some(time) = clock.time() {
            // log::debug!("time: {time:?}");
            if let Some(base_time) = app.base_time() {
                // log::debug!("base_time: {base_time:?}");
                let runtime = time.saturating_sub(base_time);
                // log::debug!("runtime: {runtime:?}");
                return Some(Duration::from_micros(runtime.useconds()));
            }
        }
    }
    None
}

// This ensures we start at a keyframe
fn wait_for_keyframe<E, T: Stream<Item = Result<StampedData, E>> + Unpin>(
    mut stream: T,
) -> impl Stream<Item = AnyResult<StampedData>> + Unpin {
    Box::pin(async_stream::stream! {
        let mut found_key = false;
        while let Some(frame) = stream.next().await {
            if let Ok(frame) = frame {
                if frame.keyframe || found_key {
                    found_key = true;
                    yield Ok(frame);
                }
            }
        }
    })
}

// Take a stream of stamped data and release them
// in waves when a new key frame is found
// this ensure that the last frame sent is always an IFrame
fn hold_stream<E, T: Stream<Item = Result<StampedData, E>> + Unpin>(
    mut stream: T,
) -> impl Stream<Item = AnyResult<StampedData>> + Unpin {
    Box::pin(async_stream::stream! {
        let mut held_frames = vec![];
        while let Some(frame) = stream.next().await {
            if let Ok(frame) = frame {
                if frame.keyframe {
                    // Release
                    // log::debug!("Yielding: {}", held_frames.len());
                    for held_frame in held_frames.drain(..) {
                        yield Ok(held_frame);
                    }
                    // log::debug!("Yielded");
                    yield Ok(frame);
                } else {
                    //  Hold
                    held_frames.push(frame);
                }
            }
        }
    })
}

// Take a stream of stamped data pause until
// it is time to display it
fn frametime_stream<E, T: Stream<Item = Result<StampedData, E>> + Unpin>(
    mut stream: T,
) -> impl Stream<Item = AnyResult<StampedData>> + Unpin {
    Box::pin(async_stream::stream! {
        const MIN_FPS_DELTA: Duration = Duration::from_millis(1000/5);
        let mut last_release = Instant::now();
        let mut cached_prev_ts = None;
        while let Some(frame) = stream.next().await {
            if let Ok(frame) = frame {
                let curr_ts = frame.ts;
                let mut prev_ts = cached_prev_ts.unwrap_or(curr_ts);

                // Check if we have gone backwards
                if curr_ts < prev_ts {
                    // If we have reset things
                    prev_ts = curr_ts;
                }

                let delta_ts =  std::cmp::min(curr_ts - prev_ts, MIN_FPS_DELTA);
                // log::debug!("curr_ts: {curr_ts:?}, {prev_ts:?} delta_ts: {delta_ts:?}");

                sleep_until(last_release + delta_ts).await;
                last_release = Instant::now();
                cached_prev_ts = Some(curr_ts);

                yield Ok(frame);
            }
        }
    })
}

#[allow(dead_code)]
// This will take a stream and if there is a notibable lack of data
// then it will repeat the last keyframe (if there have been no
// pframes in between)
fn repeat_keyframe<E, T: Stream<Item = Result<StampedData, E>> + Unpin>(
    mut stream: T,
    fallback_time: Duration,
    frame_rate: Duration,
) -> impl Stream<Item = Result<StampedData, E>> + Unpin {
    Box::pin(async_stream::stream! {
        while let Some(frame) = stream.next().await {
            if let Ok(frame) = frame {
                if frame.keyframe {
                    // log::debug!("Key Frame");
                    let repeater = frame.clone();
                    yield Ok(frame);

                    // Wait for either timeout or a new frame
                    let mut fallback_time = fallback_time;
                    loop {
                        tokio::select!{
                            v = stream.next() => {
                                if let Some(frame) = v {
                                    if let Ok(frame) = frame {
                                        log::debug!("Key Frame: Resume");
                                        yield Ok(frame);
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            },
                            _ = sleep(fallback_time) => {
                                if fallback_time != frame_rate {
                                    // This way we only print once
                                    log::debug!("Inserting Skip Frames");
                                }
                                fallback_time = frame_rate;

                                yield Ok(repeater.clone());
                            }
                        }
                    }
                } else {
                    // P frames go through as-is
                    yield Ok(frame);
                }
            }
        }
    })
}

/// Takes a stream and sends it to an appsrc
async fn send_to_appsrc<E, T: Stream<Item = Result<StampedData, E>> + Unpin>(
    mut stream: T,
    appsrc: &AppSrc,
) -> AnyResult<()> {
    let mut rt = Duration::ZERO;
    while let Some(Ok(data)) = stream.next().await {
        check_live(appsrc)?; // Stop if appsrc is dropped
        if let Some(rt_i) = get_runtime(appsrc) {
            rt = rt_i;
        }
        let buf = {
            let mut gst_buf = gstreamer::Buffer::with_size(data.data.len()).unwrap();
            {
                let gst_buf_mut = gst_buf.get_mut().unwrap();
                // log::debug!("Setting PTS: {ts:?}, Runtime: {ts:?}");
                let time = ClockTime::from_useconds(rt.as_micros() as u64);
                gst_buf_mut.set_dts(time);
                gst_buf_mut.set_pts(time);
                let mut gst_buf_data = gst_buf_mut.map_writable().unwrap();
                gst_buf_data.copy_from_slice(data.data.as_slice());
            }
            gst_buf
        };

        match appsrc.push_buffer(buf) {
            Ok(_) => Ok(()),
            Err(FlowError::Flushing) => {
                // Buffer is full just skip
                Ok(())
            }
            Err(e) => Err(anyhow!("Error in streaming: {e:?}")),
        }?;
    }
    Ok(())
}
