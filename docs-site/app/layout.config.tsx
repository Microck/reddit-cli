import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

export const baseOptions: BaseLayoutProps = {
  nav: {
    url: '/',
    title: (
      <img src="/reddit-cli-logo.png" alt="reddit-cli" width={149} height={32} className="h-8 w-auto" />
    ),
  },
  githubUrl: 'https://github.com/Microck/reddit-cli',
};
