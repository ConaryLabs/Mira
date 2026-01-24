// crates/mira-server/src/db/teams.rs
// Team management database operations for multi-user memory sharing

use anyhow::Result;
use rusqlite::{params, Connection};

use super::Database;

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
pub fn get_team_sync(
    conn: &Connection,
    team_id: i64,
) -> rusqlite::Result<Option<Team>> {
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
pub fn get_team_by_name_sync(
    conn: &Connection,
    name: &str,
) -> rusqlite::Result<Option<Team>> {
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
pub fn list_user_teams_sync(
    conn: &Connection,
    user_identity: &str,
) -> rusqlite::Result<Vec<Team>> {
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

impl Database {
    /// Create a new team
    pub fn create_team(
        &self,
        name: &str,
        description: Option<&str>,
        created_by: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO teams (name, description, created_by) VALUES (?, ?, ?)",
            params![name, description, created_by],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a team by ID
    pub fn get_team(&self, team_id: i64) -> Result<Option<Team>> {
        let conn = self.conn();
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

    /// Get a team by name
    pub fn get_team_by_name(&self, name: &str) -> Result<Option<Team>> {
        let conn = self.conn();
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

    /// Delete a team
    pub fn delete_team(&self, team_id: i64) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM teams WHERE id = ?", [team_id])?;
        Ok(deleted > 0)
    }

    /// Add a member to a team
    pub fn add_team_member(
        &self,
        team_id: i64,
        user_identity: &str,
        role: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        let role = role.unwrap_or("member");
        conn.execute(
            "INSERT INTO team_members (team_id, user_identity, role) VALUES (?, ?, ?)
             ON CONFLICT(team_id, user_identity) DO UPDATE SET role = excluded.role",
            params![team_id, user_identity, role],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Remove a member from a team
    pub fn remove_team_member(&self, team_id: i64, user_identity: &str) -> Result<bool> {
        let conn = self.conn();
        let deleted = conn.execute(
            "DELETE FROM team_members WHERE team_id = ? AND user_identity = ?",
            params![team_id, user_identity],
        )?;
        Ok(deleted > 0)
    }

    /// Check if a user is a member of a team
    pub fn is_team_member(&self, team_id: i64, user_identity: &str) -> Result<bool> {
        let conn = self.conn();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM team_members WHERE team_id = ? AND user_identity = ?",
                params![team_id, user_identity],
                |_| Ok(true),
            )
            .unwrap_or(false);
        Ok(exists)
    }

    /// List all teams a user belongs to
    pub fn list_user_teams(&self, user_identity: &str) -> Result<Vec<Team>> {
        let conn = self.conn();
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

    /// List all members of a team
    pub fn list_team_members(&self, team_id: i64) -> Result<Vec<TeamMember>> {
        let conn = self.conn();
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

    /// Get team IDs that a user belongs to (for query filtering)
    pub fn get_user_team_ids(&self, user_identity: &str) -> Result<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT team_id FROM team_members WHERE user_identity = ?",
        )?;

        let team_ids = stmt
            .query_map([user_identity], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(team_ids)
    }

    /// Update team details
    pub fn update_team(
        &self,
        team_id: i64,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn();

        if let Some(new_name) = name {
            conn.execute(
                "UPDATE teams SET name = ? WHERE id = ?",
                params![new_name, team_id],
            )?;
        }

        if let Some(new_desc) = description {
            conn.execute(
                "UPDATE teams SET description = ? WHERE id = ?",
                params![new_desc, team_id],
            )?;
        }

        Ok(true)
    }

    /// Update a team member's role
    pub fn update_team_member_role(
        &self,
        team_id: i64,
        user_identity: &str,
        role: &str,
    ) -> Result<bool> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE team_members SET role = ? WHERE team_id = ? AND user_identity = ?",
            params![role, team_id, user_identity],
        )?;
        Ok(updated > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_team_operations() {
        let db = Database::open_in_memory().expect("Failed to open in-memory db");

        // Create a team
        let team_id = db
            .create_team("Test Team", Some("A test team"), Some("user@example.com"))
            .expect("Failed to create team");
        assert!(team_id > 0);

        // Get the team
        let team = db.get_team(team_id).expect("Failed to get team");
        assert!(team.is_some());
        let team = team.unwrap();
        assert_eq!(team.name, "Test Team");

        // Add a member
        db.add_team_member(team_id, "member@example.com", Some("member"))
            .expect("Failed to add member");

        // Check membership
        assert!(db.is_team_member(team_id, "member@example.com").unwrap());
        assert!(!db.is_team_member(team_id, "other@example.com").unwrap());

        // List members
        let members = db.list_team_members(team_id).expect("Failed to list members");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_identity, "member@example.com");

        // Remove member
        assert!(db.remove_team_member(team_id, "member@example.com").unwrap());
        assert!(!db.is_team_member(team_id, "member@example.com").unwrap());

        // Delete team
        assert!(db.delete_team(team_id).unwrap());
        assert!(db.get_team(team_id).unwrap().is_none());
    }
}
