/**
 * Shared by the three.js scene and the build-time poster projection, so both draw the same structure
 * from the same camera. Plain JavaScript so Node can import it without a build step.
 * Units are roughly metres; z runs into the screen (more negative is farther from the viewer).
 */
export const RAIL_X = 5.5;              // half the distance between the two rails
export const RAIL_Y = 7.6;              // height of the rail centre line
export const RAIL_Z_NEAR = 6;
export const RAIL_Z_FAR = -122;
export const COLUMN_X = RAIL_X + 1.9;   // portal-frame columns sit outside the rails
export const COLUMN_H = RAIL_Y + 0.9;
export const FRAME_Z = [-8, -22, -36, -50, -64, -78, -92, -106, -120];
export const MODULE_DROP = 2.6;         // cable length from rail to connector module

/** Connector modules hang from the rails at these stations. The carriage (the agent) docks with them in turn. */
export const STATIONS = [
  { id: 'filesystem',   z: -4,  side: -1, tool: 'filesystem',   action: 'read_file src/lib/auth.rs' },
  { id: 'code-editor',  z: -17, side: 1,  tool: 'code-editor',  action: 'str_replace src/lib/auth.rs' },
  { id: 'shell',        z: -31, side: -1, tool: 'shell',        action: 'cargo test --package api' },
  { id: 'github',       z: -46, side: 1,  tool: 'github',       action: 'create_pull_request' },
  { id: 'google-drive', z: -62, side: -1, tool: 'google-drive', action: 'search_files "Q3 plan"' },
  { id: 'web',          z: -79, side: 1,  tool: 'web',          action: 'fetch_url docs.rs/tokio' },
  { id: 'supabase',     z: -96, side: -1, tool: 'supabase',     action: 'execute_sql select count(*)' },
];

/** The route the carriage travels, as station indexes; it loops. */
export const ROUTE = [1, 2, 3, 1, 4, 5, 6, 3, 0, 2, 5, 4, 6, 1, 0];

export const CAMERA = { position: [-14, 5.6, 22], target: [-4, 4.6, -36], fov: 34 };
export const FOG = { near: 30, far: 150 };
