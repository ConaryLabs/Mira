// crates/mira-server/src/tools/core/teams.rs
// Team management tools for multi-user memory sharing

use crate::db::{
    add_team_member_sync, create_team_sync, get_team_by_name_sync, get_team_sync,
    is_team_member_sync, list_team_members_sync, list_user_teams_sync, remove_team_member_sync,
};
use crate::mcp::requests::TeamAction;
use crate::tools::core::ToolContext;

/// Create a new team
pub async fn team_create<C: ToolContext>(
    ctx: &C,
    name: String,
    description: Option<String>,
) -> Result<String, String> {
    let Some(user_id) = ctx.get_user_identity() else {
        return Err("Cannot create team: user identity not available".to_string());
    };

    // Check if team with same name already exists
    let name_clone = name.clone();
    let existing = ctx
        .pool()
        .run(move |conn| get_team_by_name_sync(conn, &name_clone))
        .await?;

    if existing.is_some() {
        return Err(format!("Team '{}' already exists", name));
    }

    // Create the team
    let name_clone2 = name.clone();
    let user_id_for_create = user_id.clone();
    let team_id = ctx
        .pool()
        .run(move |conn| {
            create_team_sync(
                conn,
                &name_clone2,
                description.as_deref(),
                Some(&user_id_for_create),
            )
        })
        .await?;

    // Add creator as admin
    ctx.pool()
        .run(move |conn| add_team_member_sync(conn, team_id, &user_id, Some("admin")))
        .await?;

    Ok(format!(
        "Created team '{}' (id: {}). You are now the admin.",
        name, team_id
    ))
}

/// Invite a user to a team
pub async fn team_invite<C: ToolContext>(
    ctx: &C,
    team_id: i64,
    user_identity: String,
    role: Option<String>,
) -> Result<String, String> {
    let Some(current_user) = ctx.get_user_identity() else {
        return Err("Cannot invite: your identity not available".to_string());
    };

    // Verify the team exists
    let team = ctx
        .pool()
        .run(move |conn| get_team_sync(conn, team_id))
        .await?
        .ok_or_else(|| format!("Team {} not found", team_id))?;

    // Check if current user is an admin or owner
    let members = ctx
        .pool()
        .run(move |conn| list_team_members_sync(conn, team_id))
        .await?;

    let is_admin = members
        .iter()
        .any(|m| m.user_identity == current_user && (m.role == "admin" || m.role == "owner"));

    if !is_admin {
        return Err("Only team admins can invite members".to_string());
    }

    // Add the member
    let role = role.unwrap_or_else(|| "member".to_string());
    let user_identity_clone = user_identity.clone();
    let role_clone = role.clone();
    ctx.pool()
        .run(move |conn| {
            add_team_member_sync(conn, team_id, &user_identity_clone, Some(&role_clone))
        })
        .await?;

    Ok(format!(
        "Added '{}' to team '{}' as {}",
        user_identity, team.name, role
    ))
}

/// Remove a user from a team
pub async fn team_remove<C: ToolContext>(
    ctx: &C,
    team_id: i64,
    user_identity: String,
) -> Result<String, String> {
    let Some(current_user) = ctx.get_user_identity() else {
        return Err("Cannot remove: your identity not available".to_string());
    };

    // Verify the team exists
    let team = ctx
        .pool()
        .run(move |conn| get_team_sync(conn, team_id))
        .await?
        .ok_or_else(|| format!("Team {} not found", team_id))?;

    // Check if current user is an admin (or removing themselves)
    if current_user != user_identity {
        let members = ctx
            .pool()
            .run(move |conn| list_team_members_sync(conn, team_id))
            .await?;

        let is_admin = members
            .iter()
            .any(|m| m.user_identity == current_user && (m.role == "admin" || m.role == "owner"));

        if !is_admin {
            return Err("Only team admins can remove members".to_string());
        }
    }

    // Remove the member
    let user_identity_clone = user_identity.clone();
    let removed = ctx
        .pool()
        .run(move |conn| remove_team_member_sync(conn, team_id, &user_identity_clone))
        .await?;

    if removed {
        Ok(format!(
            "Removed '{}' from team '{}'",
            user_identity, team.name
        ))
    } else {
        Ok(format!(
            "'{}' was not a member of team '{}'",
            user_identity, team.name
        ))
    }
}

/// List teams the current user belongs to
pub async fn team_list<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let Some(user_id) = ctx.get_user_identity() else {
        return Err("Cannot list teams: user identity not available".to_string());
    };

    let teams = ctx
        .pool()
        .run(move |conn| list_user_teams_sync(conn, &user_id))
        .await?;

    if teams.is_empty() {
        return Ok("You are not a member of any teams.".to_string());
    }

    let mut response = format!("Your teams ({}):\n", teams.len());
    for team in teams {
        let desc = team
            .description
            .as_ref()
            .map(|d| format!(" - {}", d))
            .unwrap_or_default();
        response.push_str(&format!("  [{}] {}{}\n", team.id, team.name, desc));
    }

    Ok(response)
}

/// List members of a team
pub async fn team_members<C: ToolContext>(ctx: &C, team_id: i64) -> Result<String, String> {
    let user_id = ctx.get_user_identity();

    // Verify the team exists
    let team = ctx
        .pool()
        .run(move |conn| get_team_sync(conn, team_id))
        .await?
        .ok_or_else(|| format!("Team {} not found", team_id))?;

    // Check if user is a member (only members can see the member list)
    if let Some(uid) = user_id {
        let uid_clone = uid.clone();
        let is_member = ctx
            .pool()
            .run(move |conn| is_team_member_sync(conn, team_id, &uid_clone))
            .await?;

        if !is_member {
            return Err("You must be a team member to view the member list".to_string());
        }
    }

    let members = ctx
        .pool()
        .run(move |conn| list_team_members_sync(conn, team_id))
        .await?;

    if members.is_empty() {
        return Ok(format!("Team '{}' has no members.", team.name));
    }

    let mut response = format!("Members of '{}' ({}):\n", team.name, members.len());
    for member in members {
        response.push_str(&format!(
            "  {} ({}) - joined {}\n",
            member.user_identity,
            member.role,
            &member.joined_at[..10] // Just the date
        ));
    }

    Ok(response)
}

/// Unified team action handler
pub async fn team<C: ToolContext>(
    ctx: &C,
    action: TeamAction,
    team_id: Option<i64>,
    name: Option<String>,
    description: Option<String>,
    user_identity: Option<String>,
    role: Option<String>,
) -> Result<String, String> {
    match action {
        TeamAction::Create => {
            let name = name.ok_or("Name is required for create action")?;
            team_create(ctx, name, description).await
        }
        TeamAction::Invite | TeamAction::Add => {
            let team_id = team_id.ok_or("team_id is required for invite action")?;
            let user_identity =
                user_identity.ok_or("user_identity is required for invite action")?;
            team_invite(ctx, team_id, user_identity, role).await
        }
        TeamAction::Remove => {
            let team_id = team_id.ok_or("team_id is required for remove action")?;
            let user_identity =
                user_identity.ok_or("user_identity is required for remove action")?;
            team_remove(ctx, team_id, user_identity).await
        }
        TeamAction::List => team_list(ctx).await,
        TeamAction::Members => {
            let team_id = team_id.ok_or("team_id is required for members action")?;
            team_members(ctx, team_id).await
        }
    }
}
