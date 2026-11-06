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
        let weights = |t: f32| {
            let (t2, t3) = (t * t, t * t * t);
            [0.5 * (-t + 2.0 * t2 - t3), 0.5 * (2.0 - 5.0 * t2 + 3.0 * t3), 0.5 * (t + 4.0 * t2 - 3.0 * t3), 0.5 * (t3 - t2)]
        };
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

/// One pyramid level: the image and its gradients.
pub struct Level {
    pub image: Gray,
    pub gx: Gray,
    pub gy: Gray,
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
