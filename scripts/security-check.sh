#!/bin/bash

# AIKit Security Check Script
# Run this script to perform basic security checks on the repository

set -e

echo "🔒 AIKit Security Check"
echo "========================"

# Check if we're in the right directory
if [ ! -f "aikit/Cargo.toml" ]; then
    echo "❌ Error: Run this script from the repository root"
    exit 1
fi

echo "✅ Repository structure verified"

# Check for security vulnerabilities in dependencies
echo ""
echo "🔍 Checking for dependency vulnerabilities..."
cd aikit
if command -v cargo-audit >/dev/null 2>&1; then
    cargo audit
    echo "✅ Cargo audit completed"
else
    echo "⚠️  cargo-audit not installed. Install with: cargo install cargo-audit"
fi
cd ..

# Check for secrets in the codebase
echo ""
echo "🔍 Checking for potential secrets..."
if command -v gitleaks >/dev/null 2>&1; then
    gitleaks detect --verbose --redact
    echo "✅ Gitleaks scan completed"
else
    echo "⚠️  gitleaks not installed. Install from: https://github.com/gitleaks/gitleaks"
fi

# Check for exposed environment files
echo ""
echo "🔍 Checking for exposed environment files..."
exposed_files=$(find . -name ".env*" -not -path "./.git/*" | grep -v ".example" || true)
if [ -n "$exposed_files" ]; then
    echo "⚠️  Found potential environment files:"
    echo "$exposed_files"
    echo "   Make sure these don't contain secrets!"
else
    echo "✅ No exposed environment files found"
fi

# Check GitHub Actions security
echo ""
echo "🔍 Checking GitHub Actions security..."
if [ -d ".github/workflows" ]; then
    echo "✅ GitHub Actions workflows found"

    # Check for potentially dangerous actions
    dangerous_actions=$(grep -r "uses:" .github/workflows/ | grep -E "(docker://|run:|script:)" | cat)
    if [ -n "$dangerous_actions" ]; then
        echo "⚠️  Found potentially dangerous actions in workflows:"
        echo "$dangerous_actions"
    else
        echo "✅ No dangerous actions found in workflows"
    fi
else
    echo "⚠️  No GitHub Actions workflows found"
fi

# Check repository permissions (requires gh CLI)
echo ""
echo "🔍 Checking repository settings..."
if command -v gh >/dev/null 2>&1; then
    echo "Repository visibility: $(gh repo view --json visibility -q .visibility)"
    echo "✅ GitHub CLI available for repository checks"
else
    echo "⚠️  GitHub CLI not installed. Install to check repository settings"
fi

echo ""
echo "🎉 Security check completed!"
echo ""
echo "Next steps:"
echo "1. Review any warnings above"
echo "2. Ensure branch protection rules are configured"
echo "3. Set up Dependabot for automated dependency updates"
echo "4. Enable security alerts in repository settings"
