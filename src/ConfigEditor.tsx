import React, { PureComponent } from 'react';
import { DataSourcePluginOptionsEditorProps } from '@grafana/data';
import { DataSourceHttpSettings } from '@grafana/ui';
import { DataSourceOptions } from './types';

interface Props extends DataSourcePluginOptionsEditorProps<DataSourceOptions> {}

interface State {}

export class ConfigEditor extends PureComponent<Props, State> {
  render() {
    const { options, onOptionsChange } = this.props;

    return (
      <DataSourceHttpSettings
        defaultUrl="http://127.0.0.1:6669"
        dataSourceConfig={options}
        showAccessOptions={true}
        onChange={onOptionsChange}
      />
    );
  }
}
