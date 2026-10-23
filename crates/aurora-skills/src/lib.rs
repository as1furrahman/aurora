//! Aurora skill core (R2): SKILL.md parsing and portability (`skill`), safe collection and
//! content hashing (`tree`), the static scanner (`scan`), and pinned fetching (`fetch`).

pub mod fetch;
mod frontmatter;
pub mod scan;
pub mod skill;
pub mod tree;
