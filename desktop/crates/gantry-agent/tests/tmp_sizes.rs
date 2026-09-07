use gantry_core::*;
use std::mem::size_of;

#[test]
fn sizes() {
    println!("Message              {}", size_of::<Message>());
    println!("ContentPart          {}", size_of::<ContentPart>());
    println!("ToolCallDto          {}", size_of::<ToolCallDto>());
    println!("Interaction          {}", size_of::<Interaction>());
    println!("AgentEvent           {}", size_of::<AgentEvent>());
    println!("AgentEventKind       {}", size_of::<AgentEventKind>());
    println!("ToolDef              {}", size_of::<ToolDef>());
    println!("Settings             {}", size_of::<Settings>());
    println!("TurnSnapshot         {}", size_of::<TurnSnapshot>());
}
