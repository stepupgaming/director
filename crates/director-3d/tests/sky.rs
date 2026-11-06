//! The clear sky model with no GPU: sunlight through the air at noon and at
//! sunset, the sky's colour from zenith to horizon, the share of daylight
//! the sky adds beside the sun, twilight and haze.

use director_3d::{sky, texel_direction, Sky, SkyOptions};

const WIDTH: usize = 256;

fn at(elevation: f64, haze: f64) -> Sky {
    sky(&SkyOptions { elevation, azimuth: 0.0, haze, ground: [0.3; 3] }, WIDTH)
}

/// The sky's mean radiance over texels whose direction passes `keep`.
fn mean(s: &Sky, keep: impl Fn([f64; 3]) -> bool) -> [f64; 3] {
    let (mut sum, mut n) = ([0.0; 3], 0.0);
    for (i, t) in s.texels.iter().enumerate() {
        if keep(texel_direction(s.width, s.height, i % s.width, i / s.width)) {
            for c in 0..3 {
                sum[c] += t[c] as f64;
            }
            n += 1.0;
        }
    }
    assert!(n > 0.0);
    sum.map(|v| v / n)
}

/// The upper hemisphere's irradiance on a horizontal surface.
fn sky_light(s: &Sky) -> [f64; 3] {
    let mut e = [0.0; 3];
    for (i, t) in s.texels.iter().enumerate() {
        let (x, y) = (i % s.width, i / s.width);
        let d = texel_direction(s.width, s.height, x, y);
        if d[1] > 0.0 {
            let theta = (y as f64 + 0.5) / s.height as f64 * std::f64::consts::PI;
            let solid = (2.0 * std::f64::consts::PI / s.width as f64) * (std::f64::consts::PI / s.height as f64) * theta.sin();
            for c in 0..3 {
                e[c] += t[c] as f64 * d[1] * solid;
            }
        }
    }
    e
}

fn luma(c: [f64; 3]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[test]
fn sunlight_through_the_air_reddens_towards_the_horizon() {
    let noon = at(90.0, 1.0);
    eprintln!("sunlight at the zenith {:?}", noon.irradiance);
    // Air and haze take about 7% of red and a quarter of blue overhead.
    assert!((0.9..0.96).contains(&noon.irradiance[0]), "{:?}", noon.irradiance);
    assert!((0.7..0.8).contains(&noon.irradiance[2]), "{:?}", noon.irradiance);
    let low = at(2.0, 1.0);
    eprintln!("sunlight 2 degrees up {:?}", low.irradiance);
    assert!(low.irradiance[0] > 4.0 * low.irradiance[2], "a low sun shines red");
    assert!(low.irradiance[0] < noon.irradiance[0]);
    assert_eq!(at(-3.0, 1.0).irradiance, [0.0; 3], "a set sun sends no direct light");
}

#[test]
fn the_sky_is_blue_overhead_and_paler_at_the_horizon() {
    let s = at(40.0, 1.0);
    let zenith = mean(&s, |d| d[1] > 0.95);
    // Away from the sun, so the bright glow around it stays out.
    let horizon = mean(&s, |d| (0.02..0.08).contains(&d[1]) && d[2] < -0.5);
    eprintln!("zenith {zenith:?}, horizon opposite the sun {horizon:?}");
    assert!(zenith[2] > 2.0 * zenith[0], "the zenith is blue");
    assert!(horizon[2] / horizon[0] < zenith[2] / zenith[0], "the horizon is paler");
    assert!(luma(horizon) > luma(zenith), "the horizon is brighter");
    let sun = mean(&s, |d| d[1] > 0.0 && (d[0] * 0.0 + d[1] * s.sun[1] + d[2] * s.sun[2]) > 0.995);
    assert!(luma(sun) > 3.0 * luma(zenith), "haze glows around the sun");
    let ground = mean(&s, |d| d[1] < -0.5);
    eprintln!("ground {ground:?}");
    let lit = (s.irradiance[1] * s.sun[1] + luma(sky_light(&s))) * 0.3 / std::f64::consts::PI;
    assert!((luma(ground) - lit).abs() < 0.15 * lit, "the ground shows its albedo under the sun and sky");
}

#[test]
fn the_sky_adds_a_tenth_to_a_fifth_of_daylight() {
    for elevation in [30.0, 60.0] {
        let s = at(elevation, 1.0);
        let diffuse = luma(sky_light(&s));
        let direct = luma(s.irradiance) * s.sun[1];
        eprintln!("sun {elevation} degrees up: sky {diffuse:.4}, sun {direct:.4}, share {:.3}", diffuse / (diffuse + direct));
        let share = diffuse / (diffuse + direct);
        assert!((0.08..0.25).contains(&share), "a clear sky's share of daylight is {share}");
    }
}

#[test]
fn twilight_glows_after_sunset_and_haze_whitens_the_sky() {
    let day = luma(sky_light(&at(30.0, 1.0)));
    let dusk = luma(sky_light(&at(-3.0, 1.0)));
    eprintln!("sky light at 30 degrees {day:.4}, 3 degrees below the horizon {dusk:.6}");
    assert!(dusk > 0.0 && dusk < day / 20.0, "twilight is dim but not black");
    let clear = mean(&at(40.0, 1.0), |d| d[1] > 0.95);
    let hazy = mean(&at(40.0, 8.0), |d| d[1] > 0.95);
    eprintln!("zenith clear {clear:?}, hazy {hazy:?}");
    assert!(hazy[2] / hazy[0] < clear[2] / clear[0], "haze whitens the sky");
    let thick = at(40.0, 8.0).irradiance;
    assert!(luma(thick) < luma(at(40.0, 1.0).irradiance), "haze dims the sun");
}
