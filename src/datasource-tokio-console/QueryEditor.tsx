import { defaults } from 'lodash';

import React, { ChangeEvent, PureComponent } from 'react';
import { LegacyForms } from '@grafana/ui';
import { QueryEditorProps } from '@grafana/data';
import { DataSource } from './datasource';
import { defaultQuery, DataSourceOptions, ConsoleQuery } from './types';

const { FormField } = LegacyForms;

type Props = QueryEditorProps<DataSource, ConsoleQuery, DataSourceOptions>;

export class QueryEditor extends PureComponent<Props> {
  onStreamPathChange = (event: ChangeEvent<HTMLInputElement>) => {
    const { onChange, query, onRunQuery } = this.props;
    onChange({ ...query, streamPath: event.target.value });
    onRunQuery();
  };

  render() {
    const query = defaults(this.props.query, defaultQuery);
    const { streamPath } = query;
    return (
      <div className="gf-form">
        <FormField width={4} value={streamPath} onChange={this.onStreamPathChange} label="Path" />
      </div>
    );
  }
}
