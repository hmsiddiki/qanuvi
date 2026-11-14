//! The plan runner's pure logic (24z step A, R1): ported from `src/lib/core/decider.ts` and `src/lib/plan/*.ts`, so the
//! runner can move into the core and go on without the window. Plain functions with their tests; `compare` runs the
//! same inputs through both and must match the TypeScript results before anything TypeScript is removed.

pub mod compare;
pub mod costs;
pub mod decider;
pub mod env;
pub mod finishing;
pub mod guidance;
pub mod holdask;
pub mod housekeeping;
pub mod js;
pub mod limits;
pub mod packet;
pub mod picks;
pub mod planner;
pub mod planstate;
pub mod preflight;
pub mod prompts;
pub mod routing;
pub mod runner;
pub mod setaside;
pub mod tasklist;
