use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

use crate::state;

impl state::Document {
    pub fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let mut file = File::create(path)?;
        let encoded = postcard::to_stdvec(self)?;
        file.write_all(&encoded)?;
        Ok(())
    }

    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let data = fs::read(path)?;
        let document = postcard::from_bytes(&data)?;
        Ok(document)
    }
}
