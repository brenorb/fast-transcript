from pathlib import Path
import re

from setuptools import find_packages, setup
from setuptools.command.bdist_wheel import bdist_wheel as _bdist_wheel


REPO_ROOT = Path(__file__).resolve().parent


def cargo_version() -> str:
    cargo_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo_toml, re.MULTILINE)
    if match is None:
        raise RuntimeError("could not read version from Cargo.toml")
    return match.group(1)


class bdist_wheel(_bdist_wheel):
    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self):
        _, _, plat = super().get_tag()
        return "py3", "none", plat


setup(
    name="fscript",
    version=cargo_version(),
    description="Fast local transcription for large lectures with NVIDIA Parakeet ONNX",
    long_description=(REPO_ROOT / "README.md").read_text(encoding="utf-8"),
    long_description_content_type="text/markdown",
    author="Breno Brito",
    url="https://github.com/brenorb/fast-transcript",
    project_urls={
        "Homepage": "https://github.com/brenorb/fast-transcript",
        "Repository": "https://github.com/brenorb/fast-transcript",
        "Issues": "https://github.com/brenorb/fast-transcript/issues",
    },
    license="MIT",
    package_dir={"": "python"},
    packages=find_packages(where="python"),
    package_data={"fscript": ["bin/*"]},
    include_package_data=True,
    python_requires=">=3.9",
    entry_points={"console_scripts": ["fscript=fscript._cli:main"]},
    cmdclass={"bdist_wheel": bdist_wheel},
    zip_safe=False,
    classifiers=[
        "Development Status :: 4 - Beta",
        "Environment :: Console",
        "Intended Audience :: Developers",
        "Operating System :: MacOS",
        "Operating System :: POSIX :: Linux",
        "Programming Language :: Python :: 3",
        "Programming Language :: Rust",
        "Topic :: Multimedia :: Sound/Audio :: Speech",
        "Topic :: Utilities",
    ],
)
