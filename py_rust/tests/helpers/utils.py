import subprocess


def run(args: list[str]) -> str:
    """Run an arbitrary command, returning stdout and err combined. Raises ValueError on non-zero exit code."""
    p1 = subprocess.run(args, capture_output=True, text=True)
    total_output = f"{p1.stdout}\n{p1.stderr}".strip()
    if p1.returncode != 0:
        raise ValueError(total_output)
    else:
        print(total_output)
    return total_output
