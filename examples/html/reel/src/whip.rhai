// Whip-pan transition clip for the Director reel: street slides out, profile slides in.
// 8 shutter samples at a 300 degree shutter give the pan real motion blur.
let movie = new_director(1920, 1080, 30);
movie.configure_motion_blur(8, 300.0);
let scene = movie.add_scene(0.8);
let root = scene.add_box(#{ width: 1920.0, height: 1080.0, bg_color: "#000000" });
let strip = root.add_box(#{ width: 3840.0, height: 1080.0, position: "absolute", top: 0.0, left: 0.0, flex_direction: "row" });
strip.add_image("examples/html/reel/assets/street.jpg", #{ width: 1920.0, height: 1080.0 });
strip.add_image("examples/html/reel/assets/profile.jpg", #{ width: 1920.0, height: 1080.0 });
strip.animate("x", 0.0, -1920.0, 0.8, "cubic_bezier(0.8,0,0.2,1)");
movie
