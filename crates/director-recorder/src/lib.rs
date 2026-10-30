//! The GSAP recorder helper. It runs a page's scripts against a
//! `director-dom` document, seeks their timelines and sends back what they
//! write, asking the compiler that spawned it for layout, computed style,
//! matrices, path geometry, media queries and beats over stdin and stdout.
//! It links neither Skia nor anything that does, so a JavaScript engine can
//! link beside it.

mod engine;
mod natives;

use director_dom::protocol::{self, Answer, Ask, Finish, Load, Reply, Span};
use director_dom::Prober;
use natives::{Page, Spot};
use std::collections::HashMap;
use std::process::ExitCode;

/// Serve one run: read a [`Load`] and a [`Span`], record, and send `Ask::Done`.
pub fn serve() -> ExitCode {
    let mut input = std::io::stdin().lock();
    let start = protocol::read::<Load>(&mut input).and_then(|load| Ok((load, protocol::read::<Span>(&mut input)?)));
    drop(input);
    let (load, span) = match start {
        Ok(start) => start,
        Err(e) => {
            eprintln!("director-recorder: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Page scripts recurse deeply.
    let run = std::thread::Builder::new().stack_size(64 << 20).spawn(move || record(load, span));
    let done = match run.map(|thread| thread.join()) {
        Ok(Ok(done)) => done,
        _ => Err("the GSAP recorder panicked".into()),
    };
    match protocol::write(&mut std::io::stdout().lock(), &Ask::Done(done)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn record(load: Load, span: Span) -> Result<Finish, String> {
    let (html, _) = director_dom::load(load.page)?;
    let doc = Page::new(html, prober(), load.viewport);
    let (run, doc) = natives::with_installed(doc, || engine::execute(&load.scripts, &span.times, span.from));
    let (records, draws) = run?;
    let page = span.keep.then(|| doc.save(|spot| spot.node));
    Ok(Finish { records, draws, page })
}

/// Ask the compiler one question and wait for the answer.
pub(crate) fn ask(question: Ask) -> Answer {
    protocol::write(&mut std::io::stdout().lock(), &question).map_err(|e| format!("the GSAP recorder pipe broke: {e}"))?;
    protocol::read::<Answer>(&mut std::io::stdin().lock()).map_err(|e| format!("the GSAP recorder pipe broke: {e}"))?
}

/// Probes through the compiler, each box with its arena index in the probe.
fn prober() -> Prober<Spot> {
    Box::new(|html| {
        let ids: Vec<_> = html.tree.nodes().map(|n| n.id()).collect();
        let Reply::Placed(list) = ask(Ask::Probe(director_dom::save(html)))? else {
            return Err("the compiler sent an unexpected answer".into());
        };
        let mut map = HashMap::with_capacity(list.len());
        for (node, placed) in list {
            let id = *ids.get(node as usize).ok_or("a probed node out of range")?;
            map.insert(id, Spot { node, placed });
        }
        Ok(map)
    })
}
