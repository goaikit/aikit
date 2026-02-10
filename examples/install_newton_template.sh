#!/bin/bash
# Example: Installing a Newton template
#
# This example demonstrates how to install a Newton template package
# using the aikit install command.

set -e

# Create a temporary project directory for this example
PROJECT_DIR=$(mktemp -d)
echo "Using temporary project directory: $PROJECT_DIR"

cd "$PROJECT_DIR"

# Install the Newton template from a local path
# In production, you would use a GitHub URL like:
#   aikit install gonewton/newton-templates --ai newton --yes

# For this example, we use the local fixture path
FIXTURE_PATH="${CARGO_MANIFEST_DIR:-.}/tests/fixtures/newton-template"

echo "Installing Newton template from: $FIXTURE_PATH"

aikit install "$FIXTURE_PATH" --ai newton --yes

# Verify the installation
echo ""
echo "Verifying installation..."

if [ -d ".newton" ]; then
    echo "✓ .newton/ directory created"
else
    echo "✗ .newton/ directory not found"
    exit 1
fi

if [ -f ".newton/README.md" ]; then
    echo "✓ .newton/README.md exists"
else
    echo "✗ .newton/README.md not found"
    exit 1
fi

if [ -d ".newton/scripts" ]; then
    echo "✓ .newton/scripts/ directory exists"
else
    echo "✗ .newton/scripts/ not found"
    exit 1
fi

# Check for all expected scripts
SCRIPTS=("advisor.sh" "evaluator.sh" "post-success.sh" "post-failure.sh")
for script in "${SCRIPTS[@]}"; do
    if [ -f ".newton/scripts/$script" ]; then
        echo "✓ .newton/scripts/$script exists"
    else
        echo "✗ .newton/scripts/$script not found"
        exit 1
    fi
done

echo ""
echo "Installation successful! Newton template is ready to use."
echo ""
echo "Installed structure:"
ls -la .newton/
echo ""
echo "Scripts:"
ls -la .newton/scripts/

# Cleanup
cd /
rm -rf "$PROJECT_DIR"
echo ""
echo "Cleanup complete."
