//! Inventory drift guards: the per-axis counts, the shape uniqueness, the partition of
//! the leaf space, and the agreement between this table and the classify family tables
//! it records. Every expectation is typed out by hand; a test that derived its numbers
//! from the table would pass on any drift.

use super::*;

mod columns;
mod skill;
mod specimens;
mod table;
