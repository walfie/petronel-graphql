use image::imageops::FilterType;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

const SIZE: usize = 32;
const SMALL_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageHash(pub(crate) i64);

impl ImageHash {
    pub fn new(img: &DynamicImage) -> Self {
        ImageHash(get_hash(img))
    }

    pub fn as_i64(&self) -> i64 {
        self.0
    }
}

impl From<i64> for ImageHash {
    fn from(value: i64) -> ImageHash {
        ImageHash(value)
    }
}

// Adapted from the gbf-raidfinder implementation:
// https://github.com/walfie/gbf-raidfinder/blob/master/server/src/main/java/com/pastebin/Pj9d8jt5/ImagePHash.java
//
// ...which was adapted from a Java implementation:
// http://pastebin.com/Pj9d8jt5
fn get_hash(img: &DynamicImage) -> i64 {
    let gray = img
        .resize_exact(SIZE as u32, SIZE as u32, FilterType::Nearest)
        .to_luma();

    let mut vals = [[0.0; SIZE]; SIZE];
    for (x, y, p) in gray.enumerate_pixels() {
        vals[x as usize][y as usize] = p.0[0] as f64;
    }

    let dct_vals = apply_dct(&vals);

    let dct_slice = dct_vals
        .iter()
        .take(SMALL_SIZE)
        .flat_map(|arr| &arr[0..SMALL_SIZE])
        .cloned()
        .collect::<Vec<f64>>();

    let total: f64 = dct_slice.iter().skip(1).sum();

    let average = total / (SMALL_SIZE * SMALL_SIZE - 1) as f64;

    let hash = dct_slice
        .into_iter()
        .enumerate()
        .skip(1)
        .fold(
            0,
            |acc, (i, v)| {
                if v > average {
                    acc | (1 << i)
                } else {
                    acc
                }
            },
        );

    hash
}

fn apply_dct(f: &[[f64; SIZE]; SIZE]) -> [[f64; SIZE]; SIZE] {
    use std::f64::consts::{FRAC_1_SQRT_2, PI};

    let mut out = [[0.0; SIZE]; SIZE];

    for (u, out_arr) in out.iter_mut().enumerate() {
        for (v, out_val) in out_arr.iter_mut().enumerate() {
            for (i, arr) in f.iter().enumerate() {
                for (j, val) in arr.iter().enumerate() {
                    *out_val += val
                        * (PI * u as f64 * (2 * i + 1) as f64 / (2.0 * SIZE as f64)).cos()
                        * (PI * v as f64 * (2 * j + 1) as f64 / (2.0 * SIZE as f64)).cos();
                }
            }

            if u == 0 {
                *out_val *= FRAC_1_SQRT_2
            }

            if v == 0 {
                *out_val *= FRAC_1_SQRT_2
            }

            *out_val *= 0.25;
        }
    }

    out
}
