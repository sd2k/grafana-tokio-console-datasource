import { defaults } from 'lodash';

import React, { PureComponent } from 'react';
import { Select } from '@grafana/ui';
import { QueryEditorProps, SelectableValue } from '@grafana/data';
import { DataSource } from './datasource';
import { defaultQuery, DataSourceOptions, ConsoleQuery } from './types';


type Props = QueryEditorProps<DataSource, ConsoleQuery, DataSourceOptions>;

const streamOptions = [
  { label: 'Tasks', value: 'tasks', description: 'Tasks list' },
  { label: 'Task details', value: 'task details', description: 'Task details' },
  { label: 'Resources', value: 'resources', description: 'Resources list' },
];

export class QueryEditor extends PureComponent<Props> {
  onStreamChange = (event: SelectableValue<string>) => {
    const { onChange, query, onRunQuery } = this.props;
    onChange({ ...query, stream: event.value });
    onRunQuery();
  };

  render() {
    const query = defaults(this.props.query, defaultQuery);
    const { stream } = query;
    return (
      <div className="gf-form">
        <Select
          options={streamOptions}
          value={stream}
          onChange={(v) => {
            this.onStreamChange(v);
          }}
        />
      </div>
    );
  }
}
