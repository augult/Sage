// Throwaway measurement of the fixed prompt prefix; not committed.
use presage_core::prompt::{system_prompt, tool_schemas};
use presage_core::state::Features;

fn main() {
    let variants = [("presage", Features::presage()), ("presage, auto-watch off", Features::presage().without("auto_watch").unwrap()), ("classic", Features::classic())];
    for (name, f) in variants {
        let (sys, tools) = (system_prompt(&f), tool_schemas(&f).to_string());
        println!("{name}: system {} chars, tools {} chars, about {} tokens at 4 chars/token", sys.len(), tools.len(), (sys.len() + tools.len()).div_ceil(4));
    }
}
