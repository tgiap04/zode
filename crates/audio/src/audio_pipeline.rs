use anyhow::Context as _;
use cpal::{
    DeviceDescription, DeviceId, default_host,
    traits::{DeviceTrait, HostTrait},
};
use gpui::{App, AsyncApp, BorrowAppContext, Global};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, mixer::Mixer};

mod echo_canceller;
use echo_canceller::EchoCanceller;
mod rodio_ext;
pub use rodio_ext::RodioExt;

use crate::audio_settings::LIVE_SETTINGS;


use super::SAMPLE_RATE;

pub fn init(cx: &mut App) {
    LIVE_SETTINGS.initialize(cx);
}

// TODO(jk): this is currently cached only once - we should observe and react instead
pub fn ensure_devices_initialized(cx: &mut App) {
    if cx.has_global::<AvailableAudioDevices>() {
        return;
    }
    cx.default_global::<AvailableAudioDevices>();
    let task = cx
        .background_executor()
        .spawn(async move { get_available_audio_devices() });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let devices = task.await;
        cx.update(|cx| cx.set_global(AvailableAudioDevices(devices)));
        cx.refresh();
    })
    .detach();
}

#[derive(Default)]
pub struct Audio {
    output: Option<(MixerDeviceSink, Mixer)>,
    pub echo_canceller: EchoCanceller,
}

impl Global for Audio {}

impl Audio {
    pub fn end_call(cx: &mut App) {
        cx.update_default_global(|this: &mut Self, _cx| {
            this.output.take();
        });
    }

}

pub fn open_input_stream(
    device_id: Option<DeviceId>,
) -> anyhow::Result<rodio::microphone::Microphone> {
    let builder = rodio::microphone::MicrophoneBuilder::new();
    let builder = if let Some(id) = device_id {
        // TODO(jk): upstream patch
        // if let Some(input_device) = default_host().device_by_id(id) {
        //     builder.device(input_device);
        // }
        let mut found = None;
        for input in rodio::microphone::available_inputs()? {
            if input.clone().into_inner().id()? == id {
                found = Some(builder.device(input));
                break;
            }
        }
        found.unwrap_or_else(|| builder.default_device())?
    } else {
        builder.default_device()?
    };
    let stream = builder
        .default_config()?
        .prefer_sample_rates([
            SAMPLE_RATE,
            SAMPLE_RATE.saturating_mul(rodio::nz!(2)),
            SAMPLE_RATE.saturating_mul(rodio::nz!(3)),
            SAMPLE_RATE.saturating_mul(rodio::nz!(4)),
        ])
        .prefer_channel_counts([rodio::nz!(1), rodio::nz!(2), rodio::nz!(3), rodio::nz!(4)])
        .prefer_buffer_sizes(512..)
        .open_stream()?;
    log::info!("Opened microphone: {:?}", stream.config());
    Ok(stream)
}

pub fn resolve_device(device_id: Option<&DeviceId>, input: bool) -> anyhow::Result<cpal::Device> {
    if let Some(id) = device_id {
        if let Some(device) = default_host().device_by_id(id) {
            return Ok(device);
        }
        log::warn!("Selected audio device not found, falling back to default");
    }
    if input {
        default_host()
            .default_input_device()
            .context("no audio input device available")
    } else {
        default_host()
            .default_output_device()
            .context("no audio output device available")
    }
}

pub fn open_test_output(device_id: Option<DeviceId>) -> anyhow::Result<MixerDeviceSink> {
    let device = resolve_device(device_id.as_ref(), false)?;
    DeviceSinkBuilder::from_device(device)?
        .open_stream()
        .context("Could not open output stream")
}

#[derive(Clone, Debug)]
pub struct AudioDeviceInfo {
    pub id: DeviceId,
    pub desc: DeviceDescription,
}

impl AudioDeviceInfo {
    pub fn matches_input(&self, is_input: bool) -> bool {
        if is_input {
            self.desc.supports_input()
        } else {
            self.desc.supports_output()
        }
    }

    pub fn matches(&self, id: &DeviceId, is_input: bool) -> bool {
        &self.id == id && self.matches_input(is_input)
    }
}

impl std::fmt::Display for AudioDeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.desc.name(), self.id)
    }
}

fn get_available_audio_devices() -> Vec<AudioDeviceInfo> {
    let Some(devices) = default_host().devices().ok() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let desc = device.description().ok()?;
            Some(AudioDeviceInfo { id, desc })
        })
        .collect()
}

#[derive(Default, Clone, Debug)]
pub struct AvailableAudioDevices(pub Vec<AudioDeviceInfo>);

impl Global for AvailableAudioDevices {}
