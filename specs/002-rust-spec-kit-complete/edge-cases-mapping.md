# Edge Cases Mapping

**Date**: 2025-01-27  
**Feature**: 002-rust-spec-kit-complete

This document maps each edge case from the specification to the requirements and tasks that address them.

## Edge Case Coverage

| Edge Case | Requirement(s) | Task(s) | Status |
|-----------|---------------|---------|--------|
| Corrupted or incomplete zip file | FR-002, FR-003 | T038, T039 | Covered by error handling in download/extraction |
| Network timeouts during template download | FR-002 | T038 | Covered by reqwest timeout configuration |
| Invalid JSON in .vscode/settings.json | FR-005 | T109, T111 | Explicit task for invalid JSON handling |
| Branch names exceeding GitHub's 244-byte limit | FR-028 | T130 | Task in polish phase (may need earlier if used in git init) |
| Git init fails but template extraction succeeds | FR-007 | T043 | Covered by error handling - git init is non-fatal |
| Missing template files in zip archive | FR-003 | T039, T040 | Covered by extraction error handling |
| Multiple top-level directories in zip | FR-003 | T040 | Explicit flattening logic handles this case |
| Windows vs Unix path separators | FR-027 | T129 | Cross-platform path handling task |
| Script permission setting fails on some files | FR-006 | T045 | Error handling should continue on partial failures |
| GitHub API returns HTML error pages instead of JSON | FR-002, FR-013 | T038, T082-T088 | Rate limit and API error handling |
| Package build fails partway through | FR-031 | T103-T107 | Error handling in package command |
| Invalid YAML frontmatter in command templates | FR-032, FR-041 | T095 | YAML parsing should handle invalid frontmatter |
| Template files missing during package generation | FR-039 | T097, T099 | File existence checks before copying |
| Version conflicts when release already exists | FR-044 | T123 | Explicit error handling task |
| GitHub CLI (`gh`) not available for release creation | FR-044 | T118, T119 | Detection and error handling tasks |

## Implementation Notes

### High Priority Edge Cases (Must Handle)

1. **Invalid JSON in .vscode/settings.json** (T109, T111)
   - Must gracefully handle parse errors
   - Should preserve existing file if merge fails
   - Display clear error message to user

2. **Network timeouts** (T038)
   - Configure reqwest with appropriate timeouts
   - Provide retry guidance in error messages
   - Allow user to retry operation

3. **Corrupted zip files** (T038, T039)
   - Validate zip integrity after download
   - Check file size matches expected
   - Provide clear error if extraction fails

### Medium Priority Edge Cases

4. **Multiple top-level directories** (T040)
   - Current logic flattens if exactly one directory
   - Should handle zero or multiple directories gracefully
   - May need to error or prompt user

5. **Script permission failures** (T045)
   - Continue processing other files if one fails
   - Log warnings for failed permission sets
   - Don't fail entire operation

6. **Missing template files** (T097, T099)
   - Validate all required files exist before packaging
   - Provide clear error listing missing files
   - Fail fast before creating incomplete packages

### Low Priority Edge Cases

7. **Branch name length** (T130)
   - Currently in polish phase
   - May need to move earlier if used during git init
   - Validate and truncate with warning

8. **HTML error pages from GitHub** (T082-T088)
   - Parse Content-Type header
   - Detect HTML vs JSON responses
   - Provide appropriate error messages

## Testing Strategy

Each edge case should have:
- Unit test for the specific failure scenario
- Integration test demonstrating graceful handling
- Error message verification (matches Python version)

## Gaps

No significant gaps identified. All edge cases are covered by existing requirements and tasks.

