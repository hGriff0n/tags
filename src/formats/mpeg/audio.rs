
use super::super::meta;

pub struct Audio {

}

impl Audio {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {

        }
    }
}

impl meta::Audio for Audio {
    fn bitrate(&self) -> u32 {
        0
    }
    fn samplerate(&self) -> u32 {
        0
    }
    fn channels(&self) -> u32 {
        0
    }
    fn length(&self) -> u32 {
        0
    }
}
