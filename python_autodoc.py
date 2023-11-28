"""Generate the python code reference pages and navigation. https://mkdocstrings.github.io/recipes/?h=recip#automatic-code-reference-pages."""

from pathlib import Path

import mkdocs_gen_files


def autodoc(base_py_path_str: str, out_folder_name: str):
    """Generate the python code reference pages and navigation. https://mkdocstrings.github.io/recipes/?h=recip#automatic-code-reference-pages."""
    nav = mkdocs_gen_files.Nav()  # type: ignore

    base_path = Path(base_py_path_str)
    base_nav_ignore_sections = len(base_path.parts) - 1

    all_paths = set(base_path.rglob("*.pyi"))
    # Only include py files if the corresponding pyi file doesn't exist:
    for path in set(base_path.rglob("*.py")):
        if path.with_suffix(".pyi") in all_paths:
            continue
        all_paths.add(path)

    for path in sorted(all_paths):
        # Don't accidentally something we don't want"
        if "__pycache__" in path.parts:
            continue

        # Make sure filename or any dir in the path doesn't start with an underscore:
        if any(part.startswith("_") and not part.startswith("__init__") for part in path.parts):
            continue

        module_path = path.relative_to(".").with_suffix("")
        doc_path = path.relative_to(base_py_path_str).with_suffix(".md")
        full_doc_path = Path(out_folder_name, doc_path)

        parts = tuple(module_path.parts)

        if parts[-1] == "__init__":
            parts = parts[:-1]
            doc_path = doc_path.with_name("index.md")
            full_doc_path = full_doc_path.with_name("index.md")
        elif parts[-1] == "__main__":
            continue

        if parts:
            # [base_nav_ignore_sections:] to remove the outer dirs from the doc nav:
            nav[parts[base_nav_ignore_sections:]] = doc_path.as_posix()

            with mkdocs_gen_files.open(full_doc_path, "w") as fd:
                ident = ".".join(parts)
                fd.write(f"::: {ident}")

        mkdocs_gen_files.set_edit_path(full_doc_path, path)

    with mkdocs_gen_files.open(f"{out_folder_name}/SUMMARY.md", "w") as nav_file:
        nav_file.writelines(nav.build_literate_nav())


autodoc("./py/bitbazaar", "py_ref")

autodoc("./py_rust/python/bitbazaar_rs", "py_rust_ref")
