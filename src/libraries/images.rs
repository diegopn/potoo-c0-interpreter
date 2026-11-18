use crate::{
    Diagnostic, Image, Location, RuntimeHost,
    heap::{Heap, Memory, Value},
};

use super::{Resources, invalid_arguments, resource_id};

pub(super) struct ImageData {
    width: i32,
    height: i32,
    pixels: Memory,
}

impl ImageData {
    fn image(&self) -> Image {
        let pixels = self
            .pixels
            .borrow()
            .iter()
            .map(|pixel| {
                let Value::Int(pixel) = pixel else {
                    unreachable!("image data contains integers")
                };
                *pixel
            })
            .collect();
        Image {
            width: self.width,
            height: self.height,
            pixels,
        }
    }
}

fn pixel_count(width: i32, height: i32, location: Location) -> Result<usize, Diagnostic> {
    if width <= 0 || height <= 0 {
        return Err(location.error("image dimensions must be positive"));
    }
    width
        .checked_mul(height)
        .map(|count| count as usize)
        .ok_or_else(|| location.error("image dimensions exceed the C0 size limit"))
}

fn blank_image(width: i32, height: i32, location: Location) -> Result<Image, Diagnostic> {
    let count = pixel_count(width, height, location)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| location.error("could not allocate image pixels"))?;
    pixels.resize(count, 0);
    Ok(Image {
        width,
        height,
        pixels,
    })
}

fn subimage(
    source: &ImageData,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    location: Location,
) -> Result<Image, Diagnostic> {
    let mut result = blank_image(width, height, location)?;
    let pixels = source.pixels.borrow();
    for row in 0..height {
        let source_y = i64::from(y) + i64::from(row);
        if !(0..i64::from(source.height)).contains(&source_y) {
            continue;
        }
        for column in 0..width {
            let source_x = i64::from(x) + i64::from(column);
            if (0..i64::from(source.width)).contains(&source_x) {
                let index = source_y as usize * source.width as usize + source_x as usize;
                let Value::Int(pixel) = pixels[index] else {
                    unreachable!("image pixel")
                };
                result.pixels[(row * width + column) as usize] = pixel;
            }
        }
    }
    Ok(result)
}

impl Resources {
    pub(super) fn image(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut impl RuntimeHost,
        heap: &mut Heap,
        location: Location,
    ) -> Result<Value, Diagnostic> {
        let io_error = |error| location.error(format!("{name}: {error}"));
        let new_image = match (name, args) {
            ("image_create", [Value::Int(width), Value::Int(height)]) => {
                blank_image(*width, *height, location)?
            }
            ("image_clone", [image]) => self.image_data(image, location)?.image(),
            (
                "image_subimage",
                [
                    image,
                    Value::Int(x),
                    Value::Int(y),
                    Value::Int(width),
                    Value::Int(height),
                ],
            ) => subimage(
                self.image_data(image, location)?,
                *x,
                *y,
                *width,
                *height,
                location,
            )?,
            ("image_load", [Value::String(path)]) => match host.load_image(path) {
                Ok(image) => image,
                Err(error) => return Err(io_error(error)),
            },
            ("image_save", [image, Value::String(path)]) => {
                host.save_image(path, &self.image_data(image, location)?.image())
                    .map_err(io_error)?;
                return Ok(Value::Void);
            }
            ("image_destroy", [image]) => {
                if self.images.remove(&resource_id(image, location)?).is_none() {
                    return Err(location.error("image is destroyed"));
                }
                return Ok(Value::Void);
            }
            ("image_width", [image]) => {
                return Ok(Value::Int(self.image_data(image, location)?.width));
            }
            ("image_height", [image]) => {
                return Ok(Value::Int(self.image_data(image, location)?.height));
            }
            ("image_data", [image]) => {
                return Ok(Value::Array(Some(
                    self.image_data(image, location)?.pixels.clone(),
                )));
            }
            _ => return Err(invalid_arguments(name, location)),
        };
        let Image {
            width,
            height,
            pixels,
        } = new_image;
        if pixels.len() != pixel_count(width, height, location)? {
            return Err(location.error("image pixel count does not match its dimensions"));
        }
        let pixels = heap.block(pixels.into_iter().map(Value::Int).collect());
        let (id, value) = self.resource(heap);
        self.images.insert(
            id,
            ImageData {
                width,
                height,
                pixels,
            },
        );
        Ok(value)
    }

    fn image_data(&self, image: &Value, location: Location) -> Result<&ImageData, Diagnostic> {
        self.images
            .get(&resource_id(image, location)?)
            .ok_or_else(|| location.error("image is destroyed"))
    }
}
