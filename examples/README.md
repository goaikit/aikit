# AIKit Examples

This directory contains example scripts and usage documentation for AIKit.

## Examples

### Newton Template Installation

**File**: `install_newton_template.sh`

Demonstrates how to install a Newton template package and verify the installation.

#### Usage

To install a Newton template:

```bash
# From GitHub (production use)
aikit install gonewton/newton-templates --ai newton --yes

# From local path (development/testing)
aikit install ./tests/fixtures/newton-template --ai newton --yes
```

#### What Gets Installed

The Newton template creates the following structure in your project:

```
project/
└── .newton/
    ├── README.md              # Documentation for Newton workspace
    └── scripts/
        ├── advisor.sh         # Planning phase advisor script
        ├── evaluator.sh       # Progress evaluator script
        ├── post-success.sh    # Post-success hook (batch mode)
        └── post-failure.sh    # Post-failure hook (batch mode)
```

#### Customizing Scripts

After installation, you can customize the scripts to fit your workflow:

- **advisor.sh**: Modify to provide project-specific planning guidance
- **evaluator.sh**: Add custom validation logic for plan progress
- **post-success.sh**: Add cleanup or notification tasks after successful runs
- **post-failure.sh**: Add error handling or notification tasks after failed runs

#### Running the Example

```bash
# Make the script executable
chmod +x examples/install_newton_template.sh

# Run it
./examples/install_newton_template.sh
```

## Newton Template Package Format

A Newton template is an aikit package with the following structure:

```
newton-template/
├── aikit.toml           # Package manifest
└── newton/              # Template root directory
    ├── README.md        # Template documentation
    └── scripts/         # Newton scripts
        ├── advisor.sh
        ├── evaluator.sh
        ├── post-success.sh
        └── post-failure.sh
```

The `aikit.toml` defines the artifact mapping:

```toml
[package]
name = "newton-templates"
version = "1.0.0"
description = "Newton workspace template with scripts"
authors = ["AIKit"]

[artifacts]
"newton/**" = ".newton"
```

This mapping copies everything under `newton/` to `.newton/` in the project.
