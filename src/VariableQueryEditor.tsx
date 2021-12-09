import { SelectableValue } from '@grafana/data';
import { InlineField, Select } from '@grafana/ui';
import React, { useState } from 'react';
import { VariableQuery, VariableQueryPathName } from './types';

interface VariableQueryProps {
  query: VariableQuery;
  onChange: (query: VariableQuery, path?: VariableQueryPathName) => void;
}

const pathOptions = [{ label: 'Tasks', value: VariableQueryPathName.Tasks, description: 'Query available tasks' }];

export const VariableQueryEditor: React.FC<VariableQueryProps> = ({ onChange, query }) => {
  const [state, setState] = useState(query);

  const savePath = () => {
    onChange(state, state.path);
  };

  const handleChange = (event: SelectableValue<VariableQueryPathName>) =>
    setState({
      ...state,
      path: event.value,
    });

  return (
    <InlineField label="Query kind" labelWidth={20}>
      <Select width={100} options={pathOptions} value={state.path} onChange={handleChange} onBlur={savePath} />
    </InlineField>
  );
};
