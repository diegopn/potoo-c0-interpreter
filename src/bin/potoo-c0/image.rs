use std::{
    io::{self, BufRead, Cursor, Write},
    path::Path,
    process::{Command, Stdio},
};

use potoo_c0_interpreter::Image;

pub(super) fn load(path: &Path) -> io::Result<Image> {
    let path = path.canonicalize()?;
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-pattern_type",
            "none",
            "-f",
            "image2",
            "-i",
        ])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-c:v",
            "pam",
            "-pix_fmt",
            "rgba",
            "-f",
            "image2pipe",
            "pipe:1",
        ])
        .output()
        .map_err(codec_unavailable)?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let mut input = Cursor::new(output.stdout.as_slice());
    let mut line = String::new();
    input.read_line(&mut line)?;
    if line.trim() != "P7" {
        return Err(invalid_header());
    }
    let (mut width, mut height) = (None, None);
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Err(invalid_header());
        }
        let field = line.trim();
        if field == "ENDHDR" {
            break;
        }
        if let Some((key, value)) = field.split_once(' ') {
            match key {
                "WIDTH" => width = value.parse::<i32>().ok(),
                "HEIGHT" => height = value.parse::<i32>().ok(),
                _ => {}
            }
        }
    }
    let (width, height) = width.zip(height).ok_or_else(invalid_header)?;
    let bytes = &output.stdout[input.position() as usize..];
    let expected = width
        .checked_mul(height)
        .filter(|&length| length > 0)
        .and_then(|length| (length as usize).checked_mul(4));
    if expected != Some(bytes.len()) {
        return Err(invalid_header());
    }
    let pixels = bytes
        .chunks_exact(4)
        .map(|rgba| i32::from_be_bytes([rgba[3], rgba[0], rgba[1], rgba[2]]))
        .collect();
    Ok(Image {
        width,
        height,
        pixels,
    })
}

pub(super) fn save(path: &Path, image: &Image) -> io::Result<()> {
    let path = std::path::absolute(path)?;
    let mut process = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
        ])
        .arg(format!("{}x{}", image.width, image.height))
        .args([
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "image2",
            "-update",
            "1",
        ])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(codec_unavailable)?;
    let mut input = io::BufWriter::new(process.stdin.take().expect("piped image input"));
    let write = (|| {
        for pixel in &image.pixels {
            let [alpha, red, green, blue] = pixel.to_be_bytes();
            input.write_all(&[red, green, blue, alpha])?;
        }
        input.flush()
    })();
    drop(input);
    let status = process.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!("FFmpeg exited with {status}")));
    }
    write
}

fn invalid_header() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid decoded image dimensions or pixels",
    )
}

fn codec_unavailable(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "FFmpeg is required for CLI image file I/O",
        )
    } else {
        error
    }
}
