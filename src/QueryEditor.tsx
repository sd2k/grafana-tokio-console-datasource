import { defaults } from 'lodash';

import React, { ChangeEvent, PureComponent } from 'react';
import { QueryEditorProps, SelectableValue } from '@grafana/data';
import { Input, Select } from '@grafana/ui';

import { DataSource } from './datasource';
import { defaultQuery, DataSourceOptions, ConsolePathName, ConsoleQuery } from './types';

type Props = QueryEditorProps<DataSource, ConsoleQuery, DataSourceOptions>;

const pathOptions = [
  { label: 'Tasks', value: ConsolePathName.Tasks, description: 'Tasks list' },
  { label: 'Task details', value: ConsolePathName.TaskDetails, description: 'Task details' },
  { label: 'Task histogram', value: ConsolePathName.TaskHistogram, description: 'Task histogram' },
  { label: 'Resources', value: ConsolePathName.Resources, description: 'Resources list' },
];

export class QueryEditor extends PureComponent<Props> {
  onPathChange = (event: SelectableValue<ConsolePathName>) => {
    const { onChange, query, onRunQuery } = this.props;
    onChange({ ...query, path: event.value });
    onRunQuery();
  };

  onTaskIdChange = (event: ChangeEvent<HTMLInputElement>) => {
    const { onChange, query, onRunQuery } = this.props;
    if (query.path === ConsolePathName.TaskDetails || query.path === ConsolePathName.TaskHistogram) {
      onChange({ ...query, rawTaskId: event.target.value });
      onRunQuery();
    }
  };

  render() {
    const query = defaults(this.props.query, defaultQuery);
    const { path } = query;
    return (
      <div className="gf-form">
        <Select menuShouldPortal options={pathOptions} value={path} onChange={this.onPathChange} />
        {query.path === ConsolePathName.TaskDetails || query.path === ConsolePathName.TaskHistogram ? (
          <Input value={query.rawTaskId} onChange={this.onTaskIdChange} />
        ) : null}
      </div>
    );
  }
}
