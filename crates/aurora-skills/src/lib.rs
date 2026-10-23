//! Aurora skill core (R2): SKILL.md parsing and portability (`skill`), safe collection and
//! content hashing (`tree`), the static scanner (`scan`), pinned fetching (`fetch`), install overlays (`overlay`) and install planning (`plan`).

pub mod fetch;
mod frontmatter;
pub mod overlay;
pub mod plan;
pub mod scan;
pub mod skill;
pub mod tree;
