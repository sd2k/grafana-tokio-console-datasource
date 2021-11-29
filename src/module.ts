import { AppPlugin } from '@grafana/data';

import { AppConfig } from './AppConfig';

export const plugin = new AppPlugin<{}>().addConfigPage({
  title: 'Configuration',
  icon: 'fa fa-cog',
  // @ts-expect-error
  body: AppConfig,
  id: 'configuration',
});
