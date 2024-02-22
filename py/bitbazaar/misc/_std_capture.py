import io
import sys
import typing as tp


class StdCapture(list):
    r"""Capture stdout/stderr for the duration of a with block.

    (e.g. print statements)

    Example:
    ```python
    with StdCapture(stderr=True) as out: # By default only captures stdout
        print('hello')
        sys.stderr.write('world')
    print(out)  # ['hello', 'world']
    ```
    """

    _stderr_capture: bool
    _stdout: "tp.TextIO"
    _stderr: "tp.TextIO"
    _buf: "io.StringIO"
    _out: "list[str]"

    def __init__(self, stderr: bool = False):
        """Creation of new capturer.

        By default only stdout is captured, stderr=True enables capturing stderr too.
        """
        # Prep all instance vars:
        self.stderr_capture = stderr
        self._out = []
        self._stdout = sys.stdout
        self._stderr = sys.stderr
        self._buf = io.StringIO()

    def __enter__(self) -> list[str]:
        """Entering the capturing context."""
        # Overwrite sinks which are being captured with the buffer:
        sys.stdout = self._buf
        if self.stderr_capture:
            sys.stderr = self._buf

        return self._out

    def __exit__(self, *args):  # type: ignore
        """On context exit."""
        # First reset the global streams:
        sys.stdout = self._stdout
        sys.stderr = self._stderr

        self._out.extend(self._buf.getvalue().splitlines())
