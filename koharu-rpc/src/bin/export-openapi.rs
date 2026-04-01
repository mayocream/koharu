use std::{
    env,
    fs::File,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

fn main() -> anyhow::Result<()> {
    let spec = koharu_rpc::api::openapi_spec();

    if let Some(path) = output_path() {
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &spec)?;
        writer.write_all(b"\n")?;
    } else {
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        serde_json::to_writer_pretty(&mut writer, &spec)?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}

fn output_path() -> Option<PathBuf> {
    env::args_os().nth(1).map(PathBuf::from)
}
