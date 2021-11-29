import React, { useCallback } from 'react';
import { PluginConfigPageProps, AppPluginMeta, PluginMeta } from '@grafana/data';
import { getBackendSrv } from '@grafana/runtime';
import { Legend, Button } from '@grafana/ui';

import { AppSettings } from 'types';

// const { SecretFormField, FormField } = LegacyForms;

const updatePlugin = async (pluginId: string, data: Partial<PluginMeta<AppSettings>>): Promise<unknown> => {
  const response = await getBackendSrv()
    .fetch({
      url: `/api/plugins/${pluginId}/settings`,
      method: 'POST',
      data,
    })
    .toPromise();

  return response.data;
};

export const AppConfig: React.FC<PluginConfigPageProps<AppPluginMeta>> = ({
  plugin: {
    meta: { id, enabled = false, pinned, jsonData },
  },
}) => {
  const updateAndReload = useCallback(
    async (data: Partial<PluginMeta<AppSettings>>) => {
      try {
        await updatePlugin(id, data);

        // Reloading the page as the changes made here wouldn't be propagated to the actual plugin otherwise.
        // This is not ideal, however unfortunately currently there is no supported way for updating the plugin state.
        window.location.reload();
      } catch (e) {
        console.error('Error while updating the plugin', e);
      }
    },
    [id]
  );

  const onEnable = useCallback(async () => {
    return updateAndReload({
      enabled: true,
      pinned: true,
      jsonData,
    });
  }, [jsonData, updateAndReload]);

  const onDisable = useCallback(async () => {
    return updateAndReload({
      enabled: false,
      pinned: false,
      jsonData,
    });
  }, [jsonData, updateAndReload]);

  return (
    <div className="gf-form-group">
      <Legend>Enable / Disable</Legend>
      {enabled ? (
        <div>
          <p>The plugin is currently enabled.</p>
          <Button variant="destructive" onClick={onDisable}>
            Disable plugin
          </Button>
        </div>
      ) : (
        <div>
          <p>The plugin is currently disabled.</p>
          <Button variant="primary" onClick={onEnable}>
            Enable plugin
          </Button>
        </div>
      )}
    </div>
  );
};
