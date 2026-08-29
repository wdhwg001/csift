//! End-to-end CLI tests: drives the built binary against synthetic transcripts.
//! Shared harness + fixtures live in `harness`; tests are grouped by feature.

mod agents;
mod argv;
mod contracts;
mod elicitation;
mod files;
mod harness;
mod image;
mod list;
mod plan_audit;
mod plan_whoami;
mod recover;
mod search;
mod show;
mod spanning;
mod stats;
mod targeting;
mod verbatim;
mod whoami_lane;
