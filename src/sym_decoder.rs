use std::io::Cursor;
use std::sync::OnceLock;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn codecs() -> &'static CodecRegistry {
    static REG: OnceLock<CodecRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut r = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut r);
        r.register_all::<symphonia_adapter_libopus::OpusDecoder>();
        r
    })
}

pub struct SymDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    buf: Option<SampleBuffer<i16>>,
    pos: usize,
    pub channels: u16,
    pub sample_rate: u32,
}

impl SymDecoder {
    pub fn new(data: Vec<u8>, ext: Option<&str>) -> Result<Self, Error> {
        let mss = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());

        let mut hint = Hint::new();
        if let Some(e) = ext {
            hint.with_extension(e);
        }

        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| {
                t.codec_params.codec != CODEC_TYPE_NULL
                    && (t.codec_params.sample_rate.is_some() || t.codec_params.channels.is_some())
            })
            .ok_or(Error::Unsupported("no audio track"))?;

        let track_id = track.id;
        let decoder = codecs().make(&track.codec_params, &DecoderOptions::default())?;

        let mut this = Self {
            format,
            decoder,
            track_id,
            buf: None,
            pos: 0,
            channels: 0,
            sample_rate: 0,
        };

        if !this.fill() {
            return Err(Error::DecodeError("no decodable packets"));
        }
        Ok(this)
    }

    fn fill(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => return false, 
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let need = decoded.capacity() * spec.channels.count();
                    if self.buf.as_ref().map_or(true, |b| b.capacity() < need) {
                        self.buf = Some(SampleBuffer::<i16>::new(decoded.capacity() as u64, spec));
                    }
                    if let Some(b) = self.buf.as_mut() {
                        b.copy_interleaved_ref(decoded);
                    }
                    self.pos = 0;
                    if self.channels == 0 {
                        self.channels = spec.channels.count() as u16;
                        self.sample_rate = spec.rate;
                    }
                    if self.buf.as_ref().map_or(0, |b| b.samples().len()) == 0 {
                        continue;
                    }
                    return true;
                }
                Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
                Err(_) => return false,
            }
        }
    }
}

impl Iterator for SymDecoder {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        loop {
            if let Some(b) = &self.buf {
                if let Some(s) = b.samples().get(self.pos) {
                    self.pos += 1;
                    return Some(*s);
                }
            }
            if !self.fill() {
                return None;
            }
        }
    }
}
