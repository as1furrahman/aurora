"""
Pre-mutation snapshot and filesystem state capture.
Zero third-party dependencies.
"""

import hashlib
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional


@dataclass
class PreMutationSnapshot:
    """Snapshot of filesystem state at target location prior to mutation."""

    target_path: str
    exists: bool
    is_dir: bool
    timestamp: float
    files: Dict[str, Dict[str, Any]] = field(default_factory=dict)

    @classmethod
    def capture(cls, target_path: Path) -> "PreMutationSnapshot":
        """Capture current filesystem state at target path."""
        p = target_path.resolve()
        exists = p.exists()
        is_dir = p.is_dir() if exists else False
        timestamp = time.time()
        files: Dict[str, Dict[str, Any]] = {}

        if exists and is_dir:
            for root, _, filenames in os.walk(p):
                for fname in filenames:
                    fp = Path(root) / fname
                    rel = str(fp.relative_to(p))
                    try:
                        st = os.lstat(fp)
                        if os.path.islink(fp):
                            files[rel] = {
                                "type": "symlink",
                                "target": os.readlink(fp),
                                "dev": st.st_dev,
                                "ino": st.st_ino,
                            }
                        else:
                            with open(fp, "rb") as f:
                                h = hashlib.sha256(f.read()).hexdigest()
                            files[rel] = {
                                "type": "file",
                                "sha256": h,
                                "size": st.st_size,
                                "mode": oct(st.st_mode),
                                "dev": st.st_dev,
                                "ino": st.st_ino,
                            }
                    except OSError:
                        pass

        return cls(
            target_path=str(p),
            exists=exists,
            is_dir=is_dir,
            timestamp=timestamp,
            files=files,
        )

    def matches_current(self) -> bool:
        """Verify whether current filesystem state matches this snapshot."""
        current = self.capture(Path(self.target_path))
        if self.exists != current.exists or self.is_dir != current.is_dir:
            return False
        return self.files == current.files
