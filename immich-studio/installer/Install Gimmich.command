#!/bin/bash
# Double-click on the Mac to install or update Gimmich (opens Terminal and runs the installer).
cd "$HOME" || exit 1
curl -fsSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/immich-studio/installer/install.sh -o /tmp/gimmich-install.sh \
  || curl -fsSL https://raw.githubusercontent.com/fablab503-collab/airbridge/claude/jolly-edison-ugpm94/immich-studio/installer/install.sh -o /tmp/gimmich-install.sh
bash /tmp/gimmich-install.sh
echo
read -r -p "Press Enter to close this window." _
