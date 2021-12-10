import { DataSourceInstanceSettings, MetricFindValue, StreamingFrameOptions } from '@grafana/data';
import { DataSourceWithBackend, getTemplateSrv } from '@grafana/runtime';
import { DataSourceOptions, ConsoleQuery, ConsolePathName, VariableQueryPathName, VariableQuery } from './types';

export class DataSource extends DataSourceWithBackend<ConsoleQuery, DataSourceOptions> {
  constructor(instanceSettings: DataSourceInstanceSettings<DataSourceOptions>) {
    super(instanceSettings);
  }

  applyTemplateVariables(query: ConsoleQuery): Record<string, any> {
    if (query.path === ConsolePathName.TaskDetails || query.path === ConsolePathName.TaskHistogram) {
      query.taskId = parseInt(getTemplateSrv().replace(query.rawTaskId), 10);
    }
    return query;
  }

  streamOptionsProvider = (): StreamingFrameOptions => ({ maxLength: 10000 });

  async metricFindQuery(query: VariableQuery): Promise<MetricFindValue[]> {
    if (query.path === VariableQueryPathName.Tasks) {
      const url = '/variablevalues/tasks';
      let tasks = await this.getResource(url);
      return tasks.map((taskId: number) => ({ text: taskId.toString() }));
    }
    return [];
  }
}
