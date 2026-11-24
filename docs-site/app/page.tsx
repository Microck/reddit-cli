import Link from 'next/link';
import { HomeLayout } from 'fumadocs-ui/layouts/home';
import { baseOptions } from '@/app/layout.config';

const features = [
  {
    title: 'Two data sources',
    href: '/docs/sources/',
    description: 'The live source uses the Reddit Data API and requires app credentials. The archive source uses Arctic Shift and requires no credentials.',
  },
  {
    title: 'Search and read threads',
    href: '/docs/research-workflows/',
    description: 'Search several subreddits at once. Add top comments to each result or expand hidden replies in threads from the live source.',
  },
  {
    title: 'Search offline',
    href: '/docs/store/',
    description: 'Search posts and comments in your local SQLite store.',
  },
  {
    title: 'Output for scripts and agents',
    href: '/docs/output/',
    description: 'Pipe JSON into a script or read a thread in Markdown. Print the built-in skill to give an agent the commands.',
  },
];

export default function Home() {
  return (
    <HomeLayout {...baseOptions} links={[{ text: 'Docs', url: '/docs/' }]}>
      <main className="mx-auto w-full max-w-4xl px-6 pb-20 pt-16 sm:pt-24">
        <h1>
          <img src="/reddit-cli-logo.png" alt="reddit-cli" width={477} height={102} className="h-auto max-w-full" />
        </h1>
        <p className="mt-6 text-xl text-fd-foreground">Research Reddit from the terminal with a read-only CLI.</p>
        <pre className="mt-8 w-fit max-w-full overflow-x-auto rounded-lg border bg-fd-card px-4 py-3">
          <code>npm install -g @microck/reddit-cli</code>
        </pre>
        <Link href="/docs/" className="mt-5 inline-block font-medium text-fd-primary underline underline-offset-4">
          Read the docs
        </Link>
        <div className="mt-16 grid gap-x-10 gap-y-8 border-t pt-8 sm:grid-cols-2">
          {features.map(({ title, href, description }) => (
            <section key={href}>
              <h2 className="font-semibold">
                <Link href={href} className="hover:text-fd-primary hover:underline">{title}</Link>
              </h2>
              <p className="mt-2 text-fd-muted-foreground">{description}</p>
            </section>
          ))}
        </div>
      </main>
    </HomeLayout>
  );
}
