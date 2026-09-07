//! Keeping `tool_calls` and `interactions` in step with the events log (docs/plan/06 §3): the
//! persister calls [`apply`] for every persisted event inside the same transaction.

use gantry_core::{
    AgentEvent, AgentEventKind, ChatId, InteractionPayload, InteractionResolution,
    InteractionStatus, ToolCallDto, ToolCallStatus, ToolDisplay, ToolDisplayKind,
};
use rusqlite::Connection;

use crate::{
    db::Result,
    repos::{interactions, tool_calls},
};

/// Applies one event's effect on the projections; events without one are ignored.
pub fn apply(conn: &Connection, chat_id: ChatId, e: &AgentEvent) -> Result<()> {
    match &e.event {
        AgentEventKind::ToolCallStarted {
            call_id,
            message_id,
            connector,
            connector_name,
            tool,
            model_tool_name,
        } => {
            if tool_calls::get(conn, call_id)?.is_some() {
                return Ok(());
            }
            tool_calls::insert(
                conn,
                &ToolCallDto {
                    id: call_id.clone(),
                    chat_id,
                    turn_id: e.turn_id,
                    message_id: *message_id,
                    connector: connector.clone(),
                    connector_name: connector_name.clone(),
                    tool: tool.clone(),
                    model_tool_name: model_tool_name.clone(),
                    args: serde_json::Value::Null,
                    tier: gantry_core::RiskTier::Read,
                    status: ToolCallStatus::Proposed,
                    decision_source: None,
                    display: ToolDisplay {
                        kind: ToolDisplayKind::Connector,
                        summary: String::new(),
                    },
                    result_preview: None,
                    result: None,
                    is_error: false,
                    started_at: None,
                    ended_at: None,
                    duration_ms: None,
                },
                e.ts,
            )
        }
        AgentEventKind::ToolCallReady {
            call_id,
            args,
            tier,
            display,
        } => {
            if let Some(mut c) = tool_calls::get(conn, call_id)? {
                c.args = args.clone();
                c.tier = *tier;
                c.display = display.clone();
                tool_calls::update(conn, &c)?;
            }
            Ok(())
        }
        AgentEventKind::DecisionRequested { interaction } => {
            if interactions::get(conn, interaction.id)?.is_none() {
                interactions::insert(conn, interaction)?;
            }
            let InteractionPayload::Permission { request } = &interaction.payload;
            if let Some(mut c) = tool_calls::get(conn, &request.call_id)? {
                c.status = ToolCallStatus::AwaitingDecision;
                tool_calls::update(conn, &c)?;
            }
            Ok(())
        }
        AgentEventKind::DecisionResolved {
            interaction_id,
            resolution,
            ..
        } => {
            let status = match resolution {
                InteractionResolution::Cancelled => InteractionStatus::Cancelled,
                _ => InteractionStatus::Resolved,
            };
            interactions::resolve(conn, *interaction_id, status, resolution, e.ts)?;
            Ok(())
        }
        AgentEventKind::ToolCallExecuting { call_id, source } => {
            if let Some(mut c) = tool_calls::get(conn, call_id)? {
                c.status = ToolCallStatus::Running;
                c.decision_source = Some(*source);
                c.started_at = Some(e.ts);
                tool_calls::update(conn, &c)?;
            }
            Ok(())
        }
        AgentEventKind::ToolCallCompleted {
            call_id,
            status,
            is_error,
            duration_ms,
            result_preview,
            ..
        } => {
            if let Some(mut c) = tool_calls::get(conn, call_id)? {
                c.status = *status;
                c.is_error = *is_error;
                c.result_preview = Some(result_preview.clone());
                c.ended_at = Some(e.ts);
                c.duration_ms = Some(*duration_ms);
                tool_calls::update(conn, &c)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
