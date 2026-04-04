'use client';

import Link from 'next/link';
import type { NavigationGroup } from '@/app/navigationGroups';

export default function NavGroupHubPage({ group }: { group: NavigationGroup }) {
  return (
    <div className="animate-rise rf-flat-page">
      <section className="rf-flat-section space-y-4">
        <header className="space-y-2">
          <p className="text-xs uppercase tracking-[0.18em] text-white/55">
            Rustyfin
          </p>
          <h1 className="text-2xl font-semibold sm:text-3xl">{group.title}</h1>
          <p className="max-w-3xl text-sm muted">{group.description}</p>
        </header>

        <div className="rf-flat-list">
          {group.items.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className="rf-flat-row flex items-center justify-between gap-4"
            >
              <div className="min-w-0 space-y-1">
                <p className="text-base font-semibold">{item.label}</p>
                <p className="text-sm muted">{item.description}</p>
              </div>
              <span className="rf-text-action shrink-0 text-sm">Open</span>
            </Link>
          ))}
        </div>
      </section>
    </div>
  );
}
