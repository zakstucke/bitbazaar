import os
import pathlib
import shutil
import tempfile
import typing as tp
import uuid


class TmpFileManager:
    """A context manager for managing temporary files and directories.

    Usage:
    with TmpFileManager() as manager:
        file_path = manager.tmpfile(content="Hello, temporary file!")
        dir_path = manager.tmp_dir()
    """

    root_dir: str
    files_created: int = 0
    dirs_created: int = 0

    def __init__(self):
        self.root_dir = tempfile.mkdtemp()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):  # type: ignore
        self.cleanup()

    def tmpfile(
        self,
        content: str,
        suffix: tp.Optional[str] = None,
        parent: tp.Optional[tp.Union[str, pathlib.Path]] = None,
        full_name: tp.Optional[str] = None,
    ) -> pathlib.Path:
        """Create a temporary file.

        Parameters:
        - content: The content to write to the temporary file.
        - suffix: Optional suffix to append to the temporary file. Otherwise will be created with tempfile.
        - parent: Optional directory to create the temporary file in. Otherwise will be placed in root.
        - full_name: Optional full name of the temporary file, overrides suffix. Otherwise will be created with tempfile.

        Returns:
        - The path to the created temporary file.
        """
        if parent is None:
            parent = self.root_dir

        filename: str
        if full_name is not None:
            filename = full_name
        elif suffix is not None:
            filename = str(uuid.uuid4()) + suffix
        else:
            filename = str(uuid.uuid4())

        final_path = pathlib.Path(os.path.join(parent, filename))

        with open(final_path, "w") as file:
            file.write(content)

        self.files_created += 1

        return final_path

    def tmpdir(
        self, parent: tp.Optional[str] = None, name: tp.Optional[str] = None
    ) -> pathlib.Path:
        """Create a temporary directory.

        Parameters:
        - parent: Optional directory to create the temporary directory in. Otherwise will be placed in root.

        Returns:
        - The path to the created temporary directory.
        """
        if parent is None:
            parent = self.root_dir

        final_path = pathlib.Path(os.path.join(parent, str(uuid.uuid4()) if name is None else name))
        os.mkdir(final_path)

        self.dirs_created += 1
        return final_path

    def cleanup(self):
        """Clean up created temporary files and directories."""
        shutil.rmtree(self.root_dir)
