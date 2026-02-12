<!-- docs/modules/mira-server/identity.md -->
# identity

User identity detection for multi-user memory sharing. Determines user identity through a fallback chain.

## Key Type

`UserIdentity` - Detected user identity with source tracking.

## Detection Chain

1. **Git config** - Reads `user.name` and `user.email` from git config (format: `Name <email>`)
2. **Environment** - Falls back to `MIRA_USER_ID` environment variable
3. **System user** - Falls back to system username
4. **Unknown** - If all else fails

## Identity Sources

`GitConfig`, `Environment`, `SystemUser`, `Unknown`

## Usage

Used by the `team` tool and memory scoping to attribute memories to specific users and enforce team access controls.
