//! The GSAP recorder helper `director-html` spawns once per script run.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {
    director_recorder::serve()
}
