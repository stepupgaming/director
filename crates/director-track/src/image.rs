//! Grey images, bilinear sampling and the image pyramid the tracker reads.

/// A single-channel image of luma values from 0 to 255. Pixel (x, y) sits at
/// coordinate (x, y), so the image centre is ((w - 1) / 2, (h - 1) / 2).
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
}

impl Gray {
    pub fn new(width: usize, height: usize, pixels: Vec<f32>) -> Self {
        assert_eq!(pixels.len(), width * height, "a {width}x{height} image");
        Self { width, height, pixels }
    }

    /// The pixel at (x, y), clamped to the border.
    pub fn get(&self, x: isize, y: isize) -> f32 {
        let x = x.clamp(0, self.width as isize - 1) as usize;
        let y = y.clamp(0, self.height as isize - 1) as usize;
        self.pixels[y * self.width + x]
    }

    /// Bilinear sample at (x, y), clamped to the border.
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.width - 1) as f32);
        let y = y.clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let row = |y: usize| {
            let a = self.pixels[y * self.width + x0];
            a + (self.pixels[y * self.width + x1] - a) * fx
        };
        let top = row(y0);
        top + (row(y1) - top) * fy
    }

    /// Catmull-Rom bicubic sample at (x, y), clamped to the border. It keeps
    /// the window's detail at every fractional offset, where a bilinear
    /// sample blurs it by an amount that pulls tracks toward whole pixels.
    pub fn cubic(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.width - 1) as f32);
        let y = y.clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (wx, wy) = (weights(x - x0), weights(y - y0));
        let (xi, yi) = (x0 as isize, y0 as isize);
        let mut sum = 0.0;
        for (j, wy) in wy.iter().enumerate() {
            let yy = (yi + j as isize - 1).clamp(0, self.height as isize - 1) as usize * self.width;
            let mut row = 0.0;
            for (i, wx) in wx.iter().enumerate() {
                row += wx * self.pixels[yy + (xi + i as isize - 1).clamp(0, self.width as isize - 1) as usize];
            }
            sum += wy * row;
        }
        sum
    }

    /// `cubic` at (x + dx, y + dy) for every whole offset from -`half` to
    /// `half`, row by row into `out`. Away from the border the offsets share
    /// one set of weights, applied across and then down.
    pub fn cubic_window(&self, x: f32, y: f32, half: usize, out: &mut [f32]) {
        let side = 2 * half + 1;
        assert!(side <= WINDOW_SIDE, "a window at most {WINDOW_SIDE} wide");
        let (x0, y0) = (x.floor(), y.floor());
        let (left, top) = (x0 - half as f32 - 1.0, y0 - half as f32 - 1.0);
        let reach = (side + 2) as f32;
        if !(left >= 0.0 && top >= 0.0 && left + reach <= (self.width - 1) as f32 && top + reach <= (self.height - 1) as f32) {
            let h = half as f32;
            for (k, v) in out[..side * side].iter_mut().enumerate() {
                *v = self.cubic(x + (k % side) as f32 - h, y + (k / side) as f32 - h);
            }
            return;
        }
        let (wx, wy) = (weights(x - x0), weights(y - y0));
        let (left, top) = (left as usize, top as usize);
        let mut across = [0.0f32; WINDOW_ROWS * WINDOW_SIDE];
        for r in 0..side + 3 {
            let row = &self.pixels[(top + r) * self.width + left..];
            for c in 0..side {
                let mut v = 0.0;
                for (i, w) in wx.iter().enumerate() {
                    v += w * row[c + i];
                }
                across[r * side + c] = v;
            }
        }
        for r in 0..side {
            for c in 0..side {
                let mut v = 0.0;
                for (j, w) in wy.iter().enumerate() {
                    v += w * across[(r + j) * side + c];
                }
                out[r * side + c] = v;
            }
        }
    }

    /// The 1 4 6 4 1 binomial blur in both directions.
    fn blur(&self) -> Gray {
        const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let (w, h) = (self.width, self.height);
        let mut across = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                across[y * w + x] = (0..5).map(|k| K[k] * self.get(x as isize + k as isize - 2, y as isize)).sum();
            }
        }
        let across = Gray::new(w, h, across);
        let mut down = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                down[y * w + x] = (0..5).map(|k| K[k] * across.get(x as isize, y as isize + k as isize - 2)).sum();
            }
        }
        Gray::new(w, h, down)
    }

    /// Half the size: blurred, then every second pixel.
    pub fn half(&self) -> Gray {
        let blurred = self.blur();
        let (w, h) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                pixels.push(blurred.get(2 * x as isize, 2 * y as isize));
            }
        }
        Gray::new(w, h, pixels)
    }

    /// Central-difference gradients along x and y.
    pub fn gradients(&self) -> (Gray, Gray) {
        let (w, h) = (self.width, self.height);
        let (mut gx, mut gy) = (vec![0.0; w * h], vec![0.0; w * h]);
        for y in 0..h as isize {
            for x in 0..w as isize {
                let i = y as usize * w + x as usize;
                gx[i] = 0.5 * (self.get(x + 1, y) - self.get(x - 1, y));
                gy[i] = 0.5 * (self.get(x, y + 1) - self.get(x, y - 1));
            }
        }
        (Gray::new(w, h, gx), Gray::new(w, h, gy))
    }
}

/// The widest window `cubic_window` samples, and the rows it reads.
const WINDOW_SIDE: usize = 17;
const WINDOW_ROWS: usize = WINDOW_SIDE + 3;

/// The Catmull-Rom weights of the four taps around a fraction `t`.
fn weights(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [0.5 * (-t + 2.0 * t2 - t3), 0.5 * (2.0 - 5.0 * t2 + 3.0 * t3), 0.5 * (t + 4.0 * t2 - 3.0 * t3), 0.5 * (t3 - t2)]
}

/// One pyramid level: the image and its gradients.
pub struct Level {
    pub image: Gray,
    pub gx: Gray,
    pub gy: Gray,
}

impl Level {
    /// `cubic` of the image and both gradients at (x, y), sharing one set
    /// of weights.
    pub fn cubic3(&self, x: f32, y: f32) -> [f32; 3] {
        let (w, h) = (self.image.width, self.image.height);
        let x = x.clamp(0.0, (w - 1) as f32);
        let y = y.clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (wx, wy) = (weights(x - x0), weights(y - y0));
        let (xi, yi) = (x0 as isize, y0 as isize);
        let columns: [usize; 4] = std::array::from_fn(|i| (xi + i as isize - 1).clamp(0, w as isize - 1) as usize);
        let mut sum = [0.0f32; 3];
        for (j, wy) in wy.iter().enumerate() {
            let yy = (yi + j as isize - 1).clamp(0, h as isize - 1) as usize * w;
            for (s, image) in sum.iter_mut().zip([&self.image, &self.gx, &self.gy]) {
                let mut row = 0.0;
                for (i, wx) in wx.iter().enumerate() {
                    row += wx * image.pixels[yy + columns[i]];
                }
                *s += wy * row;
            }
        }
        sum
    }
}

/// Levels from full size down, each half the one before.
pub struct Pyramid {
    pub levels: Vec<Level>,
}

impl Pyramid {
    pub fn new(image: &Gray, count: usize) -> Self {
        let mut levels = Vec::with_capacity(count);
        let mut current = image.clone();
        for i in 0..count {
            let (gx, gy) = current.gradients();
            let next = (i + 1 < count).then(|| current.half());
            levels.push(Level { image: current, gx, gy });
            match next {
                Some(next) => current = next,
                None => break,
            }
        }
        Self { levels }
    }
}
