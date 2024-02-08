import type React from "react";

/** The entry type for params each param is a tuple[string, type] where the string is the name of the param. */
export type RouteParams_T = readonly (readonly [string, "int" | "float" | "boolean" | "string"])[];

export type RouteParamsOutput_T<Params extends RouteParams_T> = {
    [K in Params[number][0]]: Extract<Params[number], readonly [K, any]>[1] extends "int" | "float"
        ? number
        : Extract<Params[number], readonly [K, any]>[1] extends "boolean"
          ? boolean
          : Extract<Params[number], readonly [K, any]>[1] extends "string"
              ? string
              : never;
};

export type InnerRouteConfig_T<ParamsOutput extends RouteParamsOutput_T<any>> = {
    build: (params: ParamsOutput) => string;
    validators: Record<string, (param: string) => unknown>;
    Comp: React.ComponentType<ParamsOutput>;
    path: string;
};

export type RouteConfig_T<ParamsOutput extends RouteParamsOutput_T<any>> =
    InnerRouteConfig_T<ParamsOutput> & {
        route: React.JSX.Element;
    };
