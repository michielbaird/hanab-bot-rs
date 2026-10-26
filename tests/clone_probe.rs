//! Diagnostic: how expensive is cloning a `Game`, and which parts dominate?

#[path = "common/mod.rs"]
mod common;
use common::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(l.size(), Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn count<T>(label: &str, f: impl Fn() -> T) {
    let (a0, b0) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
    let t = std::time::Instant::now();
    let n = 2000;
    for _ in 0..n {
        std::hint::black_box(f());
    }
    let el = t.elapsed();
    println!(
        "{label:14} {:6} allocs {:8} bytes {:6.2} us",
        (ALLOCS.load(Relaxed) - a0) / n,
        (BYTES.load(Relaxed) - b0) / n,
        el.as_secs_f64() * 1e6 / n as f64
    );
}

/// Prints allocations and time per clone of each `Game` part: `cargo test --test clone_probe -- --ignored --nocapture`.
#[test]
#[ignore = "diagnostic output only"]
fn clone_probe() {
    let mut game = setup(
        hgroup(5),
        &[
            &["xx", "xx", "xx", "xx", "xx"],
            &["r1", "y1", "g4", "b4", "p4"],
            &["r2", "y2", "g2", "b2", "p3"],
        ],
        Setup::default(),
    );
    take_turn(&mut game, "Alice clues 1 to Bob");
    take_turn_draw(&mut game, "Bob plays r1", "g1");
    take_turn(&mut game, "Cathy clues 4 to Bob");
    take_turn(&mut game, "Alice clues 2 to Cathy");
    take_turn_draw(&mut game, "Bob plays y1", "b3");
    take_turn(&mut game, "Cathy clues blue to Bob");
    count("game", || game.clone());
    count("state", || game.state.clone());
    count("players", || game.players.clone());
    count("common", || game.common.clone());
    count("meta", || game.meta.clone());
    count("deck_ids", || game.deck_ids.clone());
    count("future", || game.future.clone());
    count("notes", || game.notes.clone());
    count("last_actions", || game.last_actions.clone());
    count("move_history", || game.move_history.clone());
    count("ext", || game.ext.clone());
    count("action_list", || game.state.action_list.clone());
    count("discard_stacks", || game.state.discard_stacks.clone());
    count("hands", || game.state.hands.clone());
    count("deck", || game.state.deck.clone());
    count("names", || game.state.names.clone());
    count("player0", || game.players[0].clone());
    count("p0.thoughts", || game.players[0].thoughts.clone());
    count("p0.certain", || game.players[0].certain_map.clone());
    count("p0.possible", || game.players[0].possible_map.clone());
    count("p0.links", || game.players[0].links.clone());
}
