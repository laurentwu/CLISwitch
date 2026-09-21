import { useState } from "react";
import { useAppTheme } from "../app/ThemeProvider";
import claudeCodeIcon from "../assets/cli/claude-code.png";
import codexIcon from "../assets/cli/codex.png";
import openCodeDarkIcon from "../assets/cli/opencode-dark.svg";
import openCodeIcon from "../assets/cli/opencode.svg";
import qwenIcon from "../assets/cli/qwen.svg";
import { CLI_MARKS } from "../shared/names";
import type { CliId } from "../shared/types";

type CliIconProps = {
  cliId: CliId;
};

type CliIconAsset = {
  light: string;
  dark?: string;
};

const CLI_ICON_ASSETS: Record<CliId, CliIconAsset> = {
  "claude-code": { light: claudeCodeIcon },
  codex: { light: codexIcon },
  opencode: { light: openCodeIcon, dark: openCodeDarkIcon },
  qwen: { light: qwenIcon },
};

function CliIconImage({ cliId, src }: { cliId: CliId; src: string }) {
  const [failed, setFailed] = useState(false);

  if (failed) return CLI_MARKS[cliId];

  return (
    <img
      className={`cli-icon${cliId === "codex" ? " cli-icon-codex" : ""}`}
      src={src}
      alt=""
      draggable={false}
      onError={() => setFailed(true)}
    />
  );
}

export function CliIcon({ cliId }: CliIconProps) {
  const { resolvedTheme } = useAppTheme();
  const asset = CLI_ICON_ASSETS[cliId];
  const src = resolvedTheme === "dark" ? (asset.dark ?? asset.light) : asset.light;

  return (
    <span className="cli-mark" aria-hidden="true">
      <CliIconImage key={`${cliId}:${src}`} cliId={cliId} src={src} />
    </span>
  );
}
