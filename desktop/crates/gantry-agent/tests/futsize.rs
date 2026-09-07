use std::future::Future;
use std::mem::size_of;

fn fsize<A, F: Future>(_f: fn(A) -> F) -> usize {
    size_of::<F>()
}

#[test]
fn future_sizes() {
    println!(
        "run_turn future           = {} bytes",
        fsize(gantry_agent::runner::run_turn)
    );
    println!(
        "RunContext                = {} bytes",
        size_of::<gantry_agent::runner::RunContext>()
    );
    println!(
        "ToolSet                   = {} bytes",
        size_of::<gantry_agent::ToolSet>()
    );
    println!(
        "TurnState                 = {} bytes",
        size_of::<gantry_agent::turn_manager::TurnState>()
    );
    println!(
        "ActiveTurn                = {} bytes",
        size_of::<gantry_agent::turn_manager::ActiveTurn>()
    );
    println!(
        "LiveMessage               = {} bytes",
        size_of::<gantry_agent::turn_manager::LiveMessage>()
    );
    println!(
        "Message                   = {} bytes",
        size_of::<gantry_core::Message>()
    );
    println!(
        "ContentPart               = {} bytes",
        size_of::<gantry_core::ContentPart>()
    );
    println!(
        "ToolCallDto               = {} bytes",
        size_of::<gantry_core::ToolCallDto>()
    );
    println!(
        "AgentEvent                = {} bytes",
        size_of::<gantry_core::AgentEvent>()
    );
    println!(
        "AgentEventKind            = {} bytes",
        size_of::<gantry_core::AgentEventKind>()
    );
    println!(
        "Interaction               = {} bytes",
        size_of::<gantry_core::Interaction>()
    );
    println!(
        "Batcher                   = {} bytes",
        size_of::<gantry_agent::Batcher>()
    );
}
