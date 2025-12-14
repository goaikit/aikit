# GitHub API Contracts: AIKIT

**Date**: 2025-01-27  
**Feature**: 002-rust-spec-kit-complete

## Overview

This document defines the GitHub API integration contracts for template downloading and release management.

## Endpoint: Get Latest Release

### Request

```
GET https://api.github.com/repos/{owner}/{repo}/releases/latest
```

**Headers**:
- `Accept: application/vnd.github+json`
- `Authorization: token {token}` (optional, for authenticated requests)
- `User-Agent: aikit/{version}`

**Path Parameters**:
- `owner`: Repository owner (e.g., `aroff`)
- `repo`: Repository name (e.g., `spec-kit`)

### Response Contract

**Success** (200 OK):
```json
{
  "tag_name": "v1.0.0",
  "published_at": "2025-01-27T12:00:00Z",
  "assets": [
    {
      "name": "spec-kit-template-copilot-sh-v1.0.0.zip",
      "size": 12345,
      "browser_download_url": "https://github.com/.../download/.../spec-kit-template-copilot-sh-v1.0.0.zip"
    }
  ]
}
```

**Rate Limit Headers** (all responses):
- `X-RateLimit-Limit`: Total limit (60 unauthenticated, 5000 authenticated)
- `X-RateLimit-Remaining`: Remaining requests
- `X-RateLimit-Reset`: Unix timestamp when limit resets

**Error Responses**:
- `403 Forbidden`: Rate limit exceeded
  - Body: `{"message": "API rate limit exceeded", ...}`
  - Headers: Rate limit info + optional `Retry-After`
- `404 Not Found`: Repository or release not found
- `500 Internal Server Error`: GitHub API error

### Error Handling Contract

**Rate Limit Exceeded**:
1. Parse rate limit headers
2. Format error message with:
   - Limit, remaining, reset time
   - Troubleshooting tips
   - Suggestion to use `--github-token`
3. Exit with code 1

**Network Errors**:
1. Detect timeout/connection errors
2. Display user-friendly error message
3. Suggest retry or check network connection
4. Exit with code 1

---

## Endpoint: Download Release Asset

### Request

```
GET {browser_download_url}
```

**Headers**:
- `Accept: application/octet-stream`
- `Authorization: token {token}` (optional)
- `User-Agent: aikit/{version}`

**Note**: `browser_download_url` from release assets response.

### Response Contract

**Success** (200 OK):
- Content-Type: `application/zip`
- Body: Binary zip file content
- Content-Length: File size in bytes

**Error Responses**:
- `403 Forbidden`: Rate limit or authentication issue
- `404 Not Found`: Asset not found
- `302 Found`: Redirect (follow redirects)

### Error Handling Contract

**Download Failure**:
1. Verify Content-Type is `application/zip`
2. Verify Content-Length matches expected size
3. Handle partial downloads (corrupted files)
4. Display error with retry suggestion
5. Exit with code 1

---

## Endpoint: Create Release (via GitHub CLI)

### Contract

Uses GitHub CLI (`gh`) command, not direct API:

```bash
gh release create <version> \
  --title "Spec Kit Templates - <version_without_v>" \
  --notes-file release_notes.md \
  .genreleases/spec-kit-template-*.zip
```

**Preconditions**:
- GitHub CLI must be installed and authenticated
- Package files must exist in `.genreleases/`
- Release must not already exist

**Output Contract**:
- Success: Release created, all files attached
- Failure: Error message from `gh` command

---

## Rate Limit Handling

### Detection

1. Check `X-RateLimit-Remaining` header after each request
2. If `remaining == 0`, next request will fail
3. Parse `X-RateLimit-Reset` to calculate wait time

### Error Formatting

Rate limit errors must include:
- Current limit (60 or 5000)
- Remaining requests (0)
- Reset time (formatted as human-readable)
- Optional `Retry-After` seconds
- Troubleshooting tips:
  - Wait until reset time
  - Use `--github-token` for higher limits
  - Check token permissions

### Example Error Message

```
Error: GitHub API rate limit exceeded

Rate limit: 60/60 requests used
Reset time: 2025-01-27 15:30:00 UTC (in 42 minutes)

To resolve:
- Wait until reset time, or
- Use --github-token to increase limit to 5000/hour

Set token via:
  aikit init --github-token <token>
  or export GH_TOKEN=<token>
```

---

## Authentication

### Token Sources (precedence order)

1. `--github-token` CLI argument
2. `GH_TOKEN` environment variable
3. `GITHUB_TOKEN` environment variable

### Token Validation

- Tokens are not validated before use
- Invalid tokens will result in 401/403 errors
- Error messages should suggest checking token validity

---

## Retry Logic

**Current Contract**: No automatic retries (matches Python version)

- Rate limit errors: Fail immediately with error message
- Network errors: Fail immediately with error message
- Server errors (5xx): Fail immediately with error message

**Future Enhancement**: Could add retry logic with exponential backoff, but must be opt-in to maintain compatibility.

---

## Asset Selection

### Template Asset Filtering

From release assets, select assets matching:
- Pattern: `spec-kit-template-<agent>-<script>-v<version>.zip`
- Agent must match selected agent key
- Script must match selected script variant (`sh` or `ps`)
- Version must match requested version (or latest)

### Selection Logic

1. Filter assets by filename pattern
2. Extract agent and script from filename
3. Match against user selection
4. If multiple matches, select largest file (most complete)
5. If no match, error with suggestions

---

## Timeout Handling

### Request Timeouts

- Connection timeout: 10 seconds
- Read timeout: 30 seconds
- Total request timeout: 60 seconds

### Timeout Error Format

```
Error: Request timeout

GitHub API request timed out after 60 seconds.

To resolve:
- Check network connection
- Retry the operation
- Use --debug for detailed diagnostics
```

