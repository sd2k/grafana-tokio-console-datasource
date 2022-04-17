import React, { ChangeEvent, PureComponent } from 'react';
import { DataSourcePluginOptionsEditorProps } from '@grafana/data';
import { DataSourceOptions } from './types';
import { LegacyForms } from '@grafana/ui';

const { FormField } = LegacyForms;

interface Props extends DataSourcePluginOptionsEditorProps<DataSourceOptions> {}

interface State {}

export class ConfigEditor extends PureComponent<Props, State> {
  onURLChange(event: ChangeEvent<HTMLInputElement>) {
    const { options, onOptionsChange } = this.props;
    options.url = event.target.value;
    onOptionsChange(options);
  }

  onRetainForChange(event: ChangeEvent<HTMLInputElement>) {
    const { options, onOptionsChange } = this.props;
    const jsonData = {
      ...options.jsonData,
      retainFor: parseInt(event.target.value, 10),
    };
    onOptionsChange({ ...options, jsonData });
  }

  render() {
    const { options } = this.props;
    const { jsonData } = options;

    return (
      <div className="gf-form-group">
        <div className="gf-form">
          <FormField
            defaultValue="http://127.0.0.1:6669"
            label="URL"
            labelWidth={6}
            inputWidth={30}
            onChange={this.onURLChange}
            value={options.url}
            placeholder="The URL of the console-enabled process to connect to."
          />
        </div>
        <div className="gf-form">
          <FormField
            label="Retain for"
            labelWidth={6}
            inputWidth={30}
            onChange={this.onRetainForChange}
            value={jsonData.retainFor || ''}
            tooltip="How long to continue displaying completed tasks and dropped resources after they have been closed [default: no limit]"
            placeholder="retain for in seconds"
            type="number"
          />
        </div>
      </div>
    );
  }
}
