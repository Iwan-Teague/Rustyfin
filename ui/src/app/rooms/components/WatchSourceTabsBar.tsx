import RoomModeTabsBar from './RoomModeTabsBar';

type WatchSource = 'video' | 'youtube' | 'web';

type Props = {
  activeSource: WatchSource;
  onSwitchSource: (source: WatchSource) => void;
  switchingDisabled: boolean;
  badges: string[];
  className?: string;
};

const WATCH_SOURCE_OPTIONS: Array<{ source: WatchSource; label: string }> = [
  { source: 'video', label: 'Local Media' },
  { source: 'youtube', label: 'YouTube' },
  { source: 'web', label: 'Web' },
];

export default function WatchSourceTabsBar({
  activeSource,
  onSwitchSource,
  switchingDisabled,
  badges,
  className,
}: Props) {
  return (
    <RoomModeTabsBar
      className={className}
      activeKey={activeSource}
      onSelect={onSwitchSource}
      disabled={switchingDisabled}
      options={WATCH_SOURCE_OPTIONS.map((option) => ({
        key: option.source,
        label: option.label,
      }))}
      badges={badges}
      badgesClassName="-translate-y-[2px]"
    />
  );
}
