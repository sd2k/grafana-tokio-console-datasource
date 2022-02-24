import { DataQueryRequest, DataSourceInstanceSettings, MetricFindValue } from '@grafana/data';
import { DataSourceWithBackend, StreamingFrameAction, StreamingFrameOptions, getTemplateSrv } from '@grafana/runtime';
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

  streamOptionsProvider = (request: DataQueryRequest<ConsoleQuery>): Partial<StreamingFrameOptions> => {
    const shouldOverwrite = request.targets.some((target) => target.path === ConsolePathName.TaskHistogram);
    return { maxLength: 10000, action: shouldOverwrite ? StreamingFrameAction.Replace : StreamingFrameAction.Append };
  };

  async metricFindQuery(query: VariableQuery): Promise<MetricFindValue[]> {
    if (query.path === VariableQueryPathName.Tasks) {
      const url = '/variablevalues/tasks';
      let tasks = await this.getResource(url);
      return tasks.map((taskId: number) => ({ text: taskId.toString() }));
    }
    return [];
  }
}
