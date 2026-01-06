# Data Model: AIKit Documentation Structure

**Date**: 2025-01-06
**Purpose**: Define the structure and relationships of AIKit documentation entities

## Documentation Entity Model

### Core Entities

#### DocumentationFile
**Purpose**: Represents individual documentation files in the webdocs/ directory (Mintlify MDX format)
**Attributes**:
- `path`: String (relative path from webdocs/, e.g., "cli-reference/overview.mdx")
- `title`: String (extracted from frontmatter or filename)
- `description`: String (from frontmatter)
- `category`: DocumentationCategory enum
- `last_modified`: DateTime
- `word_count`: Integer
- `internal_links`: Array<String> (relative paths to other docs)
- `external_links`: Array<String> (URLs to external resources)
- `code_examples`: Array<CodeExample>
- `prerequisites`: Array<String> (required setup for examples)

#### DocumentationCategory
**Enum Values**:
- `overview` (README.md - main project documentation)
- `cli_reference` (cli-commands.md - all CLI command documentation)
- `configuration` (configuration.md - config files and options)
- `packages` (packages.md - package creation, building, publishing)
- `agents` (agents.md - AI agent integration details)
- `examples` (examples.md - runnable usage examples)
- `troubleshooting` (troubleshooting.md - common issues and solutions)

#### CodeExample
**Purpose**: Represents runnable code examples in documentation
**Attributes**:
- `language`: String ("bash", "toml", "yaml", "rust", etc.)
- `content`: String (the actual code)
- `description`: String (what the example demonstrates)
- `prerequisites`: Array<String> (required setup)
- `expected_output`: String (optional, what user should see)
- `external_dependencies`: Array<String> (APIs, tools, files needed)
- `runnable`: Boolean (whether example can be executed as-is)

#### ConfigurationReference
**Purpose**: Tracks configuration file references throughout documentation
**Attributes**:
- `file_path`: String (e.g., ".aikit/config.toml", "~/.aikit/config.toml")
- `usage_context`: String (where it's referenced)
- `is_primary`: Boolean (whether this is the recommended path)
- `fallback_to`: String (optional, alternative path)

#### CommandReference
**Purpose**: Tracks CLI command references and their documentation status
**Attributes**:
- `command`: String (e.g., "aikit init", "aikit install", "aikit package init")
- `exists_in_codebase`: Boolean
- `documented_in`: Array<String> (documentation file paths)
- `has_examples`: Boolean
- `syntax_correct`: Boolean
- `category`: String (init, package, install, search, etc.)

## Entity Relationships

### Documentation Dependencies
```
DocumentationFile -- contains --> CodeExample[*]
DocumentationFile -- references --> ConfigurationReference[*]
DocumentationFile -- documents --> CommandReference[*]
DocumentationFile -- links_to --> DocumentationFile[*]
```

### Command Documentation Coverage
```
CommandReference -- documented_by --> DocumentationFile
CommandReference -- exemplified_by --> CodeExample[*]
```

### Configuration Consistency
```
ConfigurationReference -- used_in --> DocumentationFile[*]
ConfigurationReference -- fallback_to --> ConfigurationReference
```

## Validation Rules

### DocumentationFile Rules
- Must have unique path
- Must belong to exactly one category
- Must have at least one internal or external link (unless root index)
- All internal links must resolve to existing DocumentationFile paths
- Code examples must specify language and prerequisites

### CodeExample Rules
- Must specify language
- Must include prerequisites if external dependencies required
- Must be syntactically correct for specified language
- Should include expected output when practical
- Runnable examples must have complete setup instructions

### CommandReference Rules
- Must correspond to actual CLI commands: init, check, install, update, remove, list, search, package (init/build/publish), release, version
- Must be documented in at least one DocumentationFile
- Must have at least one working CodeExample with proper prerequisites
- Syntax must match actual command implementation (check clap definitions)
- Package subcommands must be clearly distinguished and documented separately

### ConfigurationReference Rules
- Primary configuration paths preferred over fallbacks
- All references to same config concept must use consistent paths
- Fallback paths must be clearly marked as alternatives

## Data Flow

### Documentation Audit Process
1. **Inventory**: Scan webdocs/ directory to create DocumentationFile entities
2. **Link Analysis**: Extract and validate internal/external links
3. **Command Validation**: Cross-reference documented commands with codebase
4. **Example Testing**: Validate CodeExample prerequisites and syntax
5. **Configuration Audit**: Identify and standardize ConfigurationReference usage

### Update Process
1. **Identify Changes**: Compare current state with target state
2. **Apply Updates**: Modify DocumentationFile content
3. **Validate Changes**: Re-run validation rules
4. **Test Examples**: Verify CodeExample functionality
5. **Link Verification**: Ensure all internal links resolve

## Quality Metrics

### Coverage Metrics
- **Command Coverage**: All CLI commands documented (init, check, install, update, remove, list, search, package subcommands, release, version)
- **Example Coverage**: All documented commands have runnable examples with prerequisites
- **Configuration Coverage**: All config file paths and options documented accurately
- **Agent Coverage**: All supported AI agents documented with correct configurations

### Quality Metrics
- **Accuracy Score**: All documented commands/examples work with actual codebase
- **Completeness Score**: No missing CLI options or use cases
- **Runnable Examples**: All code examples tested and working
- **Clear Prerequisites**: External dependencies clearly documented

## Implementation Considerations

### Data Collection
- Automated scanning of webdocs/ directory for DocumentationFile creation
- Static analysis of Markdown frontmatter and link references
- Cross-referencing with Rust codebase for CommandReference validation
- Manual review for CodeExample prerequisite identification

### Validation Implementation
- Link checker for internal reference validation
- Syntax validation for CodeExample content
- Configuration path consistency analysis
- Manual testing for example runnability

### Maintenance Strategy
- Automated checks in CI/CD pipeline
- Regular audits (quarterly) to maintain quality
- Clear ownership of documentation sections
- Review process for documentation changes
