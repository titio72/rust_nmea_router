# Testing database layer

## Problem statement

This document is about testing features that involve the database. The basic principles we have to consider are:
1. tests must be predictable
2. tests must be repeatable
3. tests must be non-destructive for the production environment

These characteristics exclude the option of using the production or UAT database, or even a database shared across different dev environments. The reason is that a database that changes continuously just because it is used, negate point 1 and 2. Using the UAT or production env negates 3.

## High level solution

The idea is to us a throw-away database that is spawn when the test starts and destroyed when the test is complete.
In order to achieve point 1 (predictability), the content of the throw-away DB (Test DB hereafter) must be completely known, so the writer of the tests knows exactly what to expect and can write effective tests.
The fact that we destroy and rebuild the Test DB for each test (or test suite), guarantees point 2 (repeatability), because a test will not be affected by the fact that previous test has changed the status of the DB.
The fact the the Test DB is local and private to each test obviously guarantees point 3.

## We need infrastructure

In order to effectively write tests in this way, we need helpers to allow the writer of the test to quickly spawn a database, populate it with realistic but completely predictable test data, check the status of the database, and reset to the pristine state at any time.
The application also must be written in a way so the tests con point to a specific configuration that points to the Test DB.

### Specs of the db-related features in the NMEA Router application

1. The code must know if it is running in test or regular mode
2. When running in test mode, load test_config.json instead of config.json (if the configuration is needed)

### Specs of the Test DB helper

In this case the user is always the developer who writs the test.

1. The user can reset the Test DB. The Test DB, after reset, has all the data tables, but empty
2. The user can populate Trips using a function add_test_trip(...)
3. The user can populate the "trips" table with a preconfigured set of trips (to be added at a later stage)
4. The user can add a VesselStatus report to the database by invoking a function add_test_vessel_status(...)
5. The user must have functions to help creating realistic data
    * a function that gives the position P1 when traveling from a position P0, with bearing H, at a given speed V, for T time (use Haversine)
    * a function to generate a vector of (position, time) pairs moving from a position P0 to a position P1 at a given speed V, at interval of T seconds
6. The user can check the values of a trip in the db against test data (the best way is to add a function in db to retrieve a Trip struct by timestamp, the user then write the assertions on the Trip struct)
7. Same functionality for the VesselStatus
6. The user can populate "environmental_data" by calling a add_test_anv(...)
7. The user can check the values in teh db (same approach as for Trip adn VesselStatus)

