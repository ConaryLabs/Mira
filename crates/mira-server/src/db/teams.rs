// crates/mira-server/src/db/teams.rs
// Team management database operations for multi-user memory sharing

use rusqlite::{Connection, params};

// ═══════════════════════════════════════════════════════════════════════════════
// Sync functions for pool.interact() usage
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new team - sync version for pool.interact()
pub fn create_team_sync(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    created_by: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO teams (name, description, created_by) VALUES (?, ?, ?)",
        params![name, description, created_by],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get a team by ID - sync version for pool.interact()
pub fn get_team_sync(conn: &Connection, team_id: i64) -> rusqlite::Result<Option<Team>> {
    let team = conn
        .query_row(
            "SELECT id, name, description, created_by, created_at FROM teams WHERE id = ?",
            [team_id],
            |row| {
                Ok(Team {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_by: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .ok();
    Ok(team)
}

/// Get a team by name - sync version for pool.interact()
pub fn get_team_by_name_sync(conn: &Connection, name: &str) -> rusqlite::Result<Option<Team>> {
    let team = conn
        .query_row(
            "SELECT id, name, description, created_by, created_at FROM teams WHERE name = ?",
            [name],
            |row| {
                Ok(Team {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_by: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .ok();
    Ok(team)
}

/// Add a member to a team - sync version for pool.interact()
pub fn add_team_member_sync(
    conn: &Connection,
    team_id: i64,
    user_identity: &str,
    role: Option<&str>,
) -> rusqlite::Result<i64> {
    let role = role.unwrap_or("member");
    conn.execute(
        "INSERT INTO team_members (team_id, user_identity, role) VALUES (?, ?, ?)
         ON CONFLICT(team_id, user_identity) DO UPDATE SET role = excluded.role",
        params![team_id, user_identity, role],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Remove a member from a team - sync version for pool.interact()
pub fn remove_team_member_sync(
    conn: &Connection,
    team_id: i64,
    user_identity: &str,
) -> rusqlite::Result<bool> {
    let deleted = conn.execute(
        "DELETE FROM team_members WHERE team_id = ? AND user_identity = ?",
        params![team_id, user_identity],
    )?;
    Ok(deleted > 0)
}

/// Check if a user is a member of a team - sync version for pool.interact()
pub fn is_team_member_sync(
    conn: &Connection,
    team_id: i64,
    user_identity: &str,
) -> rusqlite::Result<bool> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM team_members WHERE team_id = ? AND user_identity = ?",
            params![team_id, user_identity],
            |_| Ok(true),
        )
        .unwrap_or(false);
    Ok(exists)
}

/// List all teams a user belongs to - sync version for pool.interact()
pub fn list_user_teams_sync(conn: &Connection, user_identity: &str) -> rusqlite::Result<Vec<Team>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.description, t.created_by, t.created_at
         FROM teams t
         JOIN team_members tm ON t.id = tm.team_id
         WHERE tm.user_identity = ?
         ORDER BY t.name",
    )?;

    let teams = stmt
        .query_map([user_identity], |row| {
            Ok(Team {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_by: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(teams)
}

/// List all members of a team - sync version for pool.interact()
pub fn list_team_members_sync(
    conn: &Connection,
    team_id: i64,
) -> rusqlite::Result<Vec<TeamMember>> {
    let mut stmt = conn.prepare(
        "SELECT id, team_id, user_identity, role, joined_at
         FROM team_members
         WHERE team_id = ?
         ORDER BY joined_at",
    )?;

    let members = stmt
        .query_map([team_id], |row| {
            Ok(TeamMember {
                id: row.get(0)?,
                team_id: row.get(1)?,
                user_identity: row.get(2)?,
                role: row.get(3)?,
                joined_at: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(members)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Database impl methods
// ═══════════════════════════════════════════════════════════════════════════════

/// Team information
#[derive(Debug, Clone)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
}

/// Team member information
#[derive(Debug, Clone)]
pub struct TeamMember {
    pub id: i64,
    pub team_id: i64,
    pub user_identity: String,
    pub role: String,
    pub joined_at: String,
}
