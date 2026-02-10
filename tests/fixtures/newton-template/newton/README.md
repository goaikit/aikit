# Newton Workspace Template

This template provides a complete Newton workspace with scripts for evaluator, advisor, post-success, and post-failure hooks.

## Scripts

The following scripts are installed in `.newton/scripts/`:

- `advisor.sh`: Advisor script for Newton's planning phase
- `evaluator.sh`: Evaluator script for validating plan progress
- `post-success.sh`: Script to run after a successful `newton run` in batch mode
- `post-failure.sh`: Script to run after a failed `newton run` in batch mode

## Usage

After installing this template with `aikit install`, you can customize these scripts to fit your project's workflow.

## Post-Success Behavior

The `post-success.sh` script is called after a successful `newton run` in batch mode:
- Exit code 0: Plan is moved to `completed/`
- Non-zero exit code: Plan is moved to `failed/`

## Post-Failure Behavior

The `post-failure.sh` script is called after a failed `newton run` in batch mode:
- After this script runs, the plan is moved to `failed/`
