import RoomModeTabsBar from './RoomModeTabsBar';

type CreateTool = 'text' | 'canvas';

type Props = {
  activeTool: CreateTool;
  onSwitchTool: (tool: CreateTool) => void;
  switchingDisabled: boolean;
  badges: string[];
  className?: string;
};

const CREATE_TOOL_OPTIONS: Array<{ tool: CreateTool; label: string }> = [
  { tool: 'text', label: 'Document' },
  { tool: 'canvas', label: 'Canvas' },
];

export default function CreateToolTabsBar({
  activeTool,
  onSwitchTool,
  switchingDisabled,
  badges,
  className,
}: Props) {
  return (
    <RoomModeTabsBar
      className={className}
      activeKey={activeTool}
      onSelect={onSwitchTool}
      disabled={switchingDisabled}
      options={CREATE_TOOL_OPTIONS.map((option) => ({
        key: option.tool,
        label: option.label,
      }))}
      badges={badges}
    />
  );
}
