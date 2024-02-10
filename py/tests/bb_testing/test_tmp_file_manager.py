from bitbazaar.testing import TmpFileManager


def test_tmp_file_manager():
    with TmpFileManager() as manager:
        auto_name = manager.tmpfile(content="Hello, temporary file!")
        assert auto_name.is_file()
        assert auto_name.read_text() == "Hello, temporary file!"

        tmpdir = manager.tmpdir()
        assert tmpdir.is_dir()

        concrete_name = manager.tmpfile(
            content="Hello, concrete temporary file!", full_name="concrete.tmp"
        )
        assert concrete_name.name == "concrete.tmp"
        assert concrete_name.is_file()
        assert concrete_name.read_text() == "Hello, concrete temporary file!"

    # Should have all been automatically cleaned up when leaving:
    assert not auto_name.exists()
    assert not tmpdir.exists()
